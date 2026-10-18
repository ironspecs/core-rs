# PLAN.md — style-system coverage accounting (2026-10-18)

The declaration-store style redesign (proposals A+B+C on substrate S1) is
LANDED at v0.6.0 (see `MIGRATION.md` for its API record; the redesign plan
itself lives in git history). This document is the **coverage accounting
plan**: every spec area the engine has deliberately set aside, bundled by
effort/popularity/priority, with the working discipline for pinning CURRENT
behavior with running proofs.

The purpose is accounting, not redesign:

1. **Know what we do.** Every set-aside area gets a running proof pinning
   the current observable behavior (usually: declaration dropped + counted,
   typed error, or defined no-effect).
2. **Nothing may crash us.** A discovered panic on boundary data is a
   repo-rule bug (AGENTS.md "Panic and boundary policy") and is hardened in
   the same change as its pin (typed error or drop — never panic), with the
   fix noted here and in the pin.
3. **The registry stays honest.** Every pin is classified (below) and
   registered in `tests/layout_proofs/README.md` in the same change. No
   production behavior change except crash-hardening and explicitly
   adjudicated divergence fixes (B1's sign rules; anything an adjudication
   forces — each recorded here).

## The phases

- **Phase 0 — file and folder rearrangement (batch 0).** The
  `layout_proofs/` suite moves to subsystem directories ahead of its
  growth. No behavior change; full suite green.
- **Phase 1 — the accounting (B1–B12).** Every item in this document gets a
  running proof pinning its current behavior. Crashes on boundary data are
  fixed immediately (typed error or drop, never panic — the B12 rule). The
  two pre-adjudicated divergence fixes (B1's sign rules) and the B5
  adjudications land here too. Exit criteria: every row pinned and
  verified, the registry current, the full suite green, this document's
  "expected current" columns corrected against reality. **No new features
  ship in Phase 1.**
- **Phase 2 — the march on-spec.** Item by item, with the pins as
  witnesses: a named consumer need opens the D6 admission (spec citation +
  Blink/Gecko algorithm reference + proof in the same change), the feature
  is implemented per the data-structure/algorithm sketches below (they are
  starting designs, revisited at admission), and its pin flips on-spec with
  the registry updated in the same change. **The flip is the acceptance
  test.** The march is slow and methodical by design: one admission per
  landing, ordered by the bundle table and consumer need. Some items may
  never be admitted (grid layout, floats) — their pins are the permanent
  record of the boundary, which is itself a deliverable of this plan.

## Conventions (read before writing any pin)

- **Pin classes** (extends the README's registry classes):
  - **divergent** (`off_spec_*`) — current behavior contradicts the spec
    within or beside the admitted vocabulary; pinned at current values.
  - **unimplemented** (`unsupported_*`) — policy-gated absence (D6 admission
    policy in `MIGRATION.md`); the pin asserts the gate behavior (dropped +
    counted, or load error) and no crash.
  - **deliberate** — crate-owned value replacing a spec default; permanent.
  - **on-spec** — accounting found the behavior already correct; pin to keep.
  - **robustness** (`robustness_*`) — crash-resistance pins over adversarial
    inputs; orthogonal to the classes above.
- **Every pin header states**: spec claim + citation, CURRENT (asserted),
  SPEC-CORRECT, ROOT CAUSE (or the D6 admission reference for `unsupported_*`).
- **Proof shape**: table-driven; public `RootDom` APIs; assert computed
  groups (`root.computed_style(...)`), load reports, metric counters
  (`declaration_drop_count`, `StyleDeclarationDropped`), layout rects,
  staged paint ops. A passing test is the no-crash proof (panics fail tests).
- **Honesty rule**: "expected current" columns are best knowledge at writing
  time; the pinning task verifies each row, and where reality differs the
  PIN records reality and this document's row is corrected in the same change.
- **Gates per landing**: fmt, clippy, workspace tests, `cargo xtask lint`.

## Batch 0 — the suite reorganization (land first)

`layout_proofs/` moves to subsystem directories mirroring the engine's
domain boundaries. Byte-identical relocations (git mv + the mod.rs tree +
README registry paths + module-header self-references); full suite green;
the structural migration policy applies (tests relocate, never rewritten).

    tests/layout_proofs/
      README.md · mod.rs · support.rs · robustness.rs        (shared root)
      style/     cascade.rs (moved) · computed_projection.rs (absorbed from tests/)
                 unsupported_values.rs · unsupported_colors.rs · unsupported_shorthands.rs
                 unsupported_properties.rs · unsupported_selectors.rs
                 cascade_tiers.rs · admitted_gaps.rs
      layout/    block_inline.rs · normal_flow.rs · content_height.rs · flex_layout.rs
                 fixed_layers.rs · replaced_elements.rs · form_control_auto_size.rs
      text/      white_space.rs · text_auto_size.rs
      widgets/   popup_menus.rs · popup_text_rows.rs
      pipeline/  stylesheet_lifecycle.rs · transitions.rs

Notes: `support.rs` stays at root so `perf_metric.rs`/`perf_allocation.rs`'s
`#[path = "layout_proofs/support.rs"]` includes don't move; `flex_layout.rs`
stays whole (splitting by batch is a later call); `off_spec`/`unsupported`/
`robustness` test-name filters are substring matches and keep working at
depth; `tests/computed_projection.rs` folds into the suite as
`style/computed_projection.rs` (one target).

**Batch 0 scope and changes.**

- **Moves (byte-identical, `git mv`):** `cascade.rs` → `style/cascade.rs`;
  `block_inline.rs`, `normal_flow.rs`, `content_height.rs`, `flex_layout.rs`,
  `fixed_layers.rs`, `replaced_elements.rs`, `form_control_auto_size.rs` →
  `layout/`; `white_space.rs`, `text_auto_size.rs` → `text/`;
  `popup_menus.rs`, `popup_text_rows.rs` → `widgets/`;
  `stylesheet_lifecycle.rs`, `transitions.rs` → `pipeline/`.
- **`tests/computed_projection.rs` absorption (the one non-byte-identical
  move):** delete the standalone target; register `computed_projection` as a
  module under `style/`; its `#[path = "layout_proofs/support.rs"] mod support;`
  include becomes `use crate::layout_proofs::support::{...}` (the same
  symbols it imports today). Flag this in the landing note.
- **Module tree:** `tests/layout_proofs/mod.rs` declares `mod style; mod
  layout; mod text; mod widgets; mod pipeline; mod support;` — each
  subdirectory gains its own `mod.rs` declaring its files (mod.rs files hold
  no test bodies, matching today's shape).
- **README (`tests/layout_proofs/README.md`):** every file citation in the
  registry table and spec-coverage map updates to the new paths
  (`style/cascade.rs`, `layout/normal_flow.rs`, `layout/replaced_elements.rs`,
  `pipeline/transitions.rs`, …).
- **Module headers:** check each moved file's `//!` header for self-path
  citations and update them (e.g. `style/cascade.rs`'s header still points
  at `tests/layout_proofs/README.md` — that reference is path-stable).
- **New proof files land in their final homes** (the `style/` files for
  B1–B11, root `robustness.rs` for B12) in their bundle's landing — never
  moved twice.
- **Gate:** full suite green with zero content changes to moved files
  (except the `computed_projection` include). No production code touched.


## The bundles (ratings + landing order)

Effort = pinning work only (S mechanical, M some verifications, L unbounded).
Popularity = real-world CSS usage frequency. Priority = drift risk × crash
risk × consumer/admission likelihood. Landing order = table order.

| # | Bundle (proof file) | Effort | Popularity | Priority | Why |
| --- | --- | --- | --- | --- | --- |
| B1 | Sign rules & numeric edges (`style/unsupported_values.rs`) | M | H | **H** | The only *divergent* rows (negative `padding`/`width`/`border-width`/`radius`/`gap`/`font-size` accepted today; CSS rejects them) + crash-adjacent numeric edges. Small grammar fixes land with the pins. |
| B2 | Units & sizing keywords (`style/unsupported_values.rs`) | M | H | **H** | Percentages, `calc()`, viewport units, `rem`, `auto`, intrinsic keywords. Highest-likelihood admission requests. |
| B3 | Box shorthands (`style/unsupported_shorthands.rs`) | S–M | H | **H** | `margin`/`padding` (the #1 shorthand in CSS), `border-width`, `inset`, `outline` — one expansion family. |
| B4 | Text consumption & admitted quirks (`style/admitted_gaps.rs`) | M | H | **H** | The named step-5 follower (`font-family`/`weight`/`style` unconsumed) + `justify`/`pre*` verification, `transform` transitions, range-`0px` pill, `pointer-events` channel. |
| B5 | Preserved candidates (`style/admitted_gaps.rs`) | S | M | **H** | Three visible-every-frame defaults awaiting adjudication: near-white initial color, `line-height: normal` → 1.0×, border initial `0`. |
| B6 | Layout-impact properties (`style/unsupported_properties.rs`) | M | H | M–H | `z-index`, `sticky`, `box-sizing`, `aspect-ratio`, `object-fit`, `float`/`clear`, `display: grid/contents/table`. |
| B7 | Color grammar (`style/unsupported_colors.rs`) | S | H | M | hex3/4, wider named colors, `currentColor`, modern `rgb()` — one grammar-widening family. |
| B8 | Selectors & at-rules (`style/unsupported_selectors.rs`) | M | M–H | M–H | `>`, `+`, `~`, `[attr]`, more pseudo-classes/elements, selector lists, `@media`/`@supports`/`@keyframes` — currently **fatal whole-load rejections**. Includes the stray-`}` adjudication. |
| B9 | Cascade tiers (`style/cascade_tiers.rs`) | M | M | M | `!important` (declaration currently dropped), custom properties/`var()`, `@layer`, animation tier, UA-sheet-workaround pin (on-spec). |
| B10 | Other shorthands & overflow axes (`style/unsupported_shorthands.rs`) | S | M | M | `font`, `background`, `text-decoration`, `all`, `overflow-x`/`overflow-y`. |
| B11 | Text/effects property tail (`style/unsupported_properties.rs`) | M–L | L–M | L | `text-decoration`, `box-shadow`, `text-overflow`, `letter-spacing`, `line-clamp`, `writing-mode`, `visibility`, `cursor`, filters, masks, table-*, column-*, … |
| B12 | Robustness (`robustness.rs`) | M–L | — | M | The no-crash safety net. **Crash discoveries jump the queue** — they become boundary-hardening fixes in the same change. |

## B1 — sign rules & numeric edges (divergent + crash-adjacent)

Current length grammar (`unset | 0 | <px> | <em>`) has **no sign check**
(verified: `parse_authored_length` accepts negatives for every property).

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `padding-left: -10px` | accepted → negative inset | illegal, CSS rejects | **divergent — fix candidate** |
| `width: -10px`, `height: -1px` | accepted | illegal (non-negative) | **divergent — fix candidate** |
| `border-top-width: -2px`, `border-radius: -4px` (longhand), `gap: -4px`, `outline-width: -2px` | accepted | illegal | **divergent — fix candidate** |
| `outline-offset: -2px` | accepted | **legal** | on-spec (pin to keep) |
| `margin-left: -10px`, `top: -5px` | accepted | legal | on-spec (already covered; keep) |
| `font-size: -10px` | accepted → negative font into text layout | illegal | **divergent + crash-adjacent — fix candidate (reject at grammar)** |
| `font-size: 0`, `font-size: 0px` | admitted → 0px font into text layout | legal | robustness (must not crash shaping/intrinsics) |
| `line-height: 0` | admitted → 1px floor (verified) | legal | on-spec (pin the floor) |
| `width: 1e400px`, `NaNpx`, `infpx`, `-infpx` | dropped+counted (non-finite rejected) | finite only | on-spec-ish; robustness |
| `order: 9223372036854775807` (i64::MAX) | accepted → clamped to i32 (verify clamp target) | valid | robustness (no crash on clamp) |
| `order: 99999999999999999999` (beyond i64) | dropped+counted | huge values valid in CSS | unimplemented |
| `width: 3.4028235e38px`, `width: 1.4e-45px` (subnormal) | accepted (finite) | valid | robustness |
| `border-radius: 10000px` on a 10px box | accepted (verify paint clamp) | CSS caps radii at render | robustness |
| `transform: translate(999999px, -999999px)` | accepted (no clamp) | valid | robustness |
| `flex-grow: -1` | dropped+counted | non-negative | on-spec (pin to keep) |
| `order: 1.5` | dropped+counted | integer | on-spec (pin to keep) |
| `opacity: 1.5`, `opacity: -0.5` | dropped+counted (range 0..=1) | CSS clamps at computed time (our strictness choice — note it) | unimplemented-ish; pin |

**B1 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_values.rs`
  (new; registered in `style/mod.rs`; holds the B1+B2 rows, one table-driven
  test per row family, ~15–20 tests).
- **Shared helper (support.rs):** add `assert_decl_drop(root, css, applied,
  dropped)` — loads a stylesheet via `theme_stylesheet_origin()`, asserts
  `StylesheetLoadReport.declarations_applied/dropped` and
  `context().declaration_drop_count()`. Most unsupported_* rows collapse to
  this one helper plus a computed-group read.
- **Production change — the sign-rule divergence fix** (the only divergence
  fixes in the accounting; adjudicated above):
  - `src/domains/style/presentation_property.rs`: split `DeclGrammar::Length`
    into `Length` (signed — margins ×4, top/right/bottom/left, outline-offset)
    and `NonNegativeLength` (padding ×4, width/height/min/max ×6,
    border-width ×4, border-radius ×4, row-gap/column-gap, outline-width);
    change `FontSize`'s grammar from `LengthNoUnset` to a non-negative form
    (`NonNegativeLengthNoUnset` or a flag — implementer's shape, the grammar
    enum is the admission table so the sign policy lives there as metadata);
    the `FlexBasis` grammar arm also rejects negative lengths (CSS: negative
    flex-basis is illegal).
  - `src/domains/style/decl_parse.rs`: the new grammar arms reject negative
    scalars (`scalar < 0.0 → None`). `parse_authored_length`
    (`src/types/dom_attr_parse.rs:116-130`) stays sign-agnostic; the policy
    lives in the grammar layer, not the primitive.
  - The resolver in `src/domains/style/computed/project.rs` needs no change
    (rejected negatives never reach it).
- **Landing order within the batch (three commits for a clean record):**
  (a) the `off_spec_*` pins of the current accepted-negative behavior (they
  pass); (b) the grammar fix (pins fail); (c) relabel the pins on-spec and
  update the registry rows in the same commit.
- **`font-size: 0`:** stays admitted (CSS-legal). The robustness pin proves
  `CosmicMetrics::new(0, …)` (`text_layout/mod.rs:189,279`) and intrinsic
  measurement survive. If cosmic panics at 0, the hardening is render-side
  (a zero-size text run renders nothing) — never a clamp that would render
  1px text (that would be wrong per CSS).
- **Numeric edges:** `order` clamp verified saturating at
  `project.rs:444-445` (`i64` clamped to `i32::MIN..=MAX`); pin it on-spec.
  Non-finite rejection (`1e400px`, `NaNpx`, `infpx`) is already enforced by
  `parse_finite_length_scalar`; pin on-spec.
- **Registry:** the B1 divergent rows (padding/width/border-width/radius/gap/
  outline-width/font-size negatives) enter as `off_spec_*`, relabel on-spec
  in commit (c); the on-spec/robustness rows register as pins with no
  divergence entry.

**B1 data structures and algorithms (Phase 1 fix, referenced for the record).**
Current state: the admission table is `DeclGrammar` in
`presentation_property.rs` — a `Copy` enum where each variant IS a grammar
family, attached per property in the static `METADATA` table. That shape is
deliberate (redesign approach C): every per-property policy is metadata in
ONE table, so a policy question ("is this length signed?") has exactly one
answer-site. The fix extends the same shape: `Length` (signed) and
`NonNegativeLength` variants, with the sign policy assigned per property in
`METADATA`; the primitive (`parse_authored_length`,
`dom_attr_parse.rs:116-130`) stays sign-agnostic — policy lives in the
grammar layer, not the primitive, because the primitive is shared by
grammars with different sign rules. Algorithm: one comparison
(`scalar < 0.0 → None`) in the two new `parse_value` arms of
`decl_parse.rs`; rejection flows through the existing drop-and-continue
report (CSS 2.1 §4.1.7 treats out-of-range as malformed). The non-finite
rejection already lives in `parse_finite_length_scalar` (NaN/inf never
enter a group — finite f32 is a group-field invariant the A7 `memcmp`-style
diffs rely on).



## B2 — units & sizing keywords

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `width: 50%`, `margin-left: 10%`, `font-size: 120%`, `line-height: 150%` | dropped+counted | percentages | unimplemented |
| `width: calc(100% - 10px)`, `calc(1px + 1px)`, `min()/max()/clamp()` | dropped+counted | math functions | unimplemented |
| `width: 100vw`, `height: 50vh`, `10vmin`, `10vmax` | dropped+counted | viewport units | unimplemented |
| `font-size: 2rem` | dropped+counted | root em | unimplemented |
| `1ex`, `2ch`, `1lh`, `1rlh` | dropped+counted | relative units | unimplemented |
| `width: auto`, `margin-top: auto` | dropped+counted (observably ≈ initial for width) | `auto` keyword | unimplemented |
| `width: min-content` / `max-content` / `fit-content` | dropped+counted | intrinsic size keywords | unimplemented |
| `width: 10` (unitless non-zero) | dropped+counted | **CSS also rejects** | on-spec (pin to keep) |
| `width: inherit` / `initial` / `revert` / `revert-layer` | dropped+counted (only `unset` admitted) | CSS-wide keywords | unimplemented |
| `font-size: unset` | dropped+counted (grammar is `LengthNoUnset`) | `unset` is CSS-wide | unimplemented |

**B2 scope and changes.**

- **Proof file:** same `style/unsupported_values.rs` as B1 (the B2 rows form
  their own table family inside it).
- **No production changes** — every row is a drop-pin through the B1
  `assert_decl_drop` helper plus a computed read
  (`root.computed_style(node).expect(...).as_box()` etc.) proving the
  property stayed at its group initial.
- **The `auto` nuance:** `width: auto` drops today, which is observably
  close to the CSS initial — the pin must assert BOTH facts explicitly (the
  declaration is dropped+counted AND the computed width is `None`), so the
  accounting never confuses "correct outcome" with "admitted grammar".
  `margin-top: auto` gets the same two-sided pin (dropped AND `margin[0] == 0.0`).
- **Percentage rows** assert the drop across the four grammar shapes
  (length box, font-size, line-height, and one inset) so a future percentage
  admission has one flip-site per shape.
- **Registry:** all rows register `unsupported_*` with the D6 admission
  reference; no divergent entries.

**B2 data structures and algorithms (Phase 2 preview).** Current state:
`DeclLength { Unset, Px(f32), Em(f32) }` (`decl_value.rs`) is the only
length shape, and computed groups store RESOLVED px (`Option<f32>`),
because A6 resolves everything resolvable at style time — that was the
redesign's deliberate choice: groups are `Copy`, compared by value, and
consumers never re-parse. Percentages break the "resolve at style time"
premise: a percentage resolves against the containing block, which exists
only at layout. The admission-time shape therefore splits
**computed vs used** (the redesign's own "used-vs-computed honesty" note,
CSS Cascade §4.5): the box group's length fields become an enum
(`BoxLength { Auto, Px(f32), Percent(f32) }` — still `Copy`, still
value-compared) holding the COMPUTED form, and layout resolves
`Percent(x)` against the containing-block dimension it already owns (CSS
Values §percentages: width → cb width; margin/padding → cb width even
vertically; font-size → parent font-size; line-height → element font-size).
`calc()` is a separate, later admission: a bounded expression type
(`CalcExpr`, depth-capped tree over our admitted units — Blink's
`MathExpressionNode`/Servo's `calc` module are the engine references),
resolved to `BoxLength` when all terms are absolute, kept symbolic
otherwise. `rem` resolves at style time against the ROOT element's
computed font-size (the pass already seeds from the root; no used-value
split needed — same shape as `em`, `DeclLength::Rem(f32)`).
`auto`/intrinsic keywords ride the existing `DeclValue::Keyword`
vocabulary: `auto` maps to the group's `None`/Auto initial (free), while
`auto` MARGINS are the honest exception — CSS resolves them at layout
(absorbing free space), so they need the same `BoxLength::Auto` used-value
resolution (the layout algorithm that distributes free space — the
centering case consumers will actually ask for). Viewport units
(`vw`/`vh`/`vmin`/`vmax`) resolve at style time against the viewport
dimensions the composite stage already knows — a style-time available
context, no used-split.



## B3 — box shorthands

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `margin: 8px`, `margin: 1px 2px 3px 4px` | dropped+counted | margin ×4 | unimplemented |
| `padding: 8px` … | dropped+counted | padding ×4 | unimplemented |
| `border: 1px`, `border-width: 1px 2px` | dropped+counted | border widths ×4 (style/color components unadmitted) | unimplemented |
| `inset: 0`, `inset: 1px 2px` | dropped+counted | top/right/bottom/left | unimplemented |
| `outline: 2px` | dropped+counted | outline-width (style/color unadmitted) | unimplemented |

**B3 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_shorthands.rs`
  (new; holds B3 and B10 rows as separate table families).
- **No production changes** — drop-pins through `assert_decl_drop`, plus a
  per-shorthand expansion-behavior pin: each row asserts the corresponding
  longhand group fields stay at initial (e.g. after `margin: 1px 2px 3px
  4px`, `computed_box().margin == [0.0; 4]`).
- **Admission notes for the future flip:** the 1–4 corner mapping is already
  implemented for `border-radius` (`decl_parse.rs` `expand_border_radius`)
  and is the template for `margin`/`padding`/`border-width`/`inset`;
  `outline` and `border` (width-only subset) follow the
  `Shorthand::BorderSide` alias pattern already in `property_shape`.
- **Registry:** `unsupported_*` rows; the margin/padding row headers note
  these are the #1 admission candidates (popularity).

**B3 data structures and algorithms (Phase 2 preview).** Current state: the
system has exactly ONE shorthand-expansion point — A1 in `decl_parse.rs`
(CSS Cascade 5 §3: "a shorthand sets all of its longhands exactly as if
expanded in place," at declaration time) — with the `Shorthand` enum +
`property_shape` name table and `expand_border_radius` as the 1–4-value
corner-mapping reference implementation. The admission of every shorthand
in this bundle and B10 is the same mechanical shape: a new `Shorthand`
variant, a name in `property_shape`, and an expansion fn returning
`SmallVec<[Declaration; 4]>`; `margin`/`padding`/`border-width`/`inset`
reuse the corner map verbatim (inset maps to top/right/bottom/left);
`outline` and `border` follow the existing `Shorthand::BorderSide`
width-only-alias precedent (the unadmitted style/color components are
dropped with a counted note or rejected per the fail-closed rule — the
call is made at admission with the pin flipping). No new data structures:
the cascade and consumers only ever see longhands, which is WHY the
expansion is safe — the winners fold, groups, and diff machinery are
shorthand-agnostic by construction (the redesign's B decision).



## B4 — text consumption & admitted quirks

Admitted grammars whose computed values are ignored, approximated, or
quirky downstream. Verify each at pin time.

| Behavior | Expected current (verify) | Spec-correct | Class |
| --- | --- | --- | --- |
| `font-family` computed but ignored by text layout (hardcoded UiSans) | text renders UiSans regardless | consume the computed family | **divergent (step-5 follower)** — pin |
| `font-weight` / `font-style` computed but ignored | rendered regular/upright | consume weight/style | divergent (same family) — pin |
| `text-align: justify`/`justified` consumption | verify: justified lines or start-like fallback | justified text | verify; pin actual |
| `font-style: oblique` | dropped+counted (closed set normal/italic) | oblique admitted in CSS | unimplemented |
| `white-space: pre` / `pre-line` / `break-spaces` consumption | admitted keywords; verify real text-layout modes | full white-space modes | verify per keyword; pin actual |
| `transform` transitions (`transition: transform 100ms`) | **no transition schedules** (jump; v0.5 had delayed step) | transform interpolates | **divergent (documented, MIGRATION.md)** — pin |
| range control + authored `border-radius: 0px` | falls to pill default (flattened presence) | authored wins over control default | **divergent (documented)** — pin; fix is the behavior-sheet tier (F) |
| `dir` content attr vs `direction` property | `direction` wins (documented) | HTML/CSS interaction | on-spec (pin to keep) |
| `pointer-events` channel | **verify first** (hit_test.rs reads a `pointer-events` channel — name it, its values) | — | verify; class accordingly |

**B4 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/admitted_gaps.rs`
  (new; holds B4 and B5 rows).
- **`font-family` unconsumed (divergent, the step-5 follower):** pin through
  text-intrinsic measurement — a container styled `font-family: serif`
  produces text intrinsic widths identical to the default (the family is
  hardcoded at `text_layout_request.rs:33-48` to `TextFontFamilyId::UiSans`,
  the one-variant enum at `text_layout/mod.rs:36-39`, resolved by
  `preferred_repo_font_family` at `text_render.rs:23-63`), while
  `root.computed_style(node).expect(...).font_family()` resolves the declared
  string through `RootDom::resolve_computed_text`. The pin asserts both:
  computed family present AND rendering unchanged. Class: **divergent**;
  consumption is the named step-5 follower.
- **`font-weight`/`font-style` unconsumed:** same shape (computed
  `as_font().weight`/`style` present; rendered run unchanged). Class: divergent.
- **`text-align: justify`:** verified CONSUMED — `text_render.rs:106` maps
  `TextAlign::Justified → cosmic_text::Align::Justified`. Pin on-spec with a
  two-word line in a constrained width (justified spacing must differ from
  start-aligned; the `text_auto_size.rs` harness provides the measurement
  pattern).
- **`font-style: oblique`:** drop-pin (closed set is normal/italic) —
  unimplemented.
- **`white-space: pre-line` / `break-spaces`:** verify each keyword's actual
  text-layout mode against the consumed `normal`/`nowrap`/`pre`/`pre-wrap`
  set (pinned in `text/white_space.rs`). If a keyword maps to the same mode
  as another (approximation), pin the approximation and classify it
  (divergent vs deliberate at pin time).
- **`transform` transitions (divergent, documented in MIGRATION.md):** pin
  that `transition: transform 100ms` with a `translateX` change presents the
  target immediately at the next update (no scheduled transition — the
  typed `Text` value cannot interpolate through the scheduler; the v0.5
  delayed-step is also gone). Assert no active transition on
  `PresentationProperty::Transform` and `computed_transform().translate_px`
  at target. Fix is typed-translate interpolation (a small admission-gated
  follow-up).
- **Range control + authored `border-radius: 0px` (divergent, documented):**
  pin the pill fallback (`element_behaviors/paint.rs` range radius reads
  `radius != [0.0; 4]` as presence). Fix belongs to the behavior-sheet tier
  (F): the pill default becomes a base behavior declaration the authored
  `0px` overrides.
- **`dir` vs `direction`:** on-spec pin (property wins over the content
  attribute; already resolved in the projection's `apply_dir_content_attribute`).
- **`pointer-events` (verified):** the channel is the content-side
  `Custom("pointer-events")` attribute, consumed by hit-testing at
  `hit_test.rs:332-336`: `"none"` → disabled, `"auto"` → enabled, anything
  else → inherit the parent's state. It is NOT in the presentation table by
  design (deliberate: it is behavior input, not a cascaded style). Pins:
  `auto`/`none` on-spec (subtree disable + re-enable); wider CSS values
  (`visiblePainted`, `painted`, `fill`, `stroke`, `all`) stored-but-inherit —
  unimplemented rows.

**B4 data structures and algorithms (Phase 2 preview).** Current state: the
computed font group already carries everything text layout needs —
`family: Option<TextKey>` (interned via the document's computed-text
interner, resolved by `RootDom::resolve_computed_text`), `weight: u16`,
`style: Keyword` — because the redesign's S1 font group was DESIGNED for
this follower (the fields exist, inherit correctly per CSS Fonts, and are
diff-classified so a family change re-measures text). The consumption work
is at the text-layout boundary: `TextFontFamilyId`
(`text_layout/mod.rs:36-39`) stops being a one-variant enum and becomes a
resolved family request — the `TextLayoutRequest` (`text_layout_request.rs`)
carries the resolved family STRING (parsed as a CSS family list:
comma-separated, quotes stripped, generic families `serif`/`sans-serif`/
`monospace`/`system-ui` mapped to fontdb generics), `weight` → cosmic
`Weight(u16)`, `style` → cosmic `FontStyle`; matching and fallback are
delegated to fontdb/cosmic-text (Blink/Gecko do the same: first matched
family in list order, generics last — the engine reference for the
fallback algorithm). The font catalog (`ui_dom_text_font_catalog.rs`) owns
family-name resolution to loaded faces; the intrinsic text measurement and
the render path consume the same resolved triple so measure == render (the
existing single-measure rule). `white-space` mode gaps (`pre-line`,
`break-spaces`) map into the shaper's collapse/preserve table
(`text_render.rs`); `pre-line` = collapse spaces, preserve newlines;
`break-spaces` = preserve trailing spaces — both are shaping-policy rows,
not new structures. The `transform`-transition follower is smaller than it
looks: the computed transform group is already `translate_px: [f32; 2]`,
so the scheduler interpolates two px channels exactly like a length pair —
the work is admitting a typed transform pair into the scheduler's
`DeclValue` interpolation table (the 3.3e note), not designing one.



## B5 — preserved pre-redesign candidates

Preserved per D8, never pinned. Divergent-vs-deliberate adjudication at pin
time, recorded here and in the registry:

| Behavior | Current | Spec default | Notes |
| --- | --- | --- | --- |
| initial `color` when nothing declared | near-white `[240/255;3, 1.0]` (design-system default) | `canvastext` (black) | likely **deliberate** (dark-theme crate) — adjudicate |
| `line-height: normal` | `1.0 × font-size` | UA-dependent, ~1.2 | divergent or deliberate — adjudicate |
| border widths when undeclared | `0px` | `medium` | divergent or deliberate (borders opt-in) — adjudicate |
| 16:9 placeholder ratio (never-loaded images) | already **deliberate**, registered | 300×150 default object size | already pinned; no work |

**B5 scope and changes.**

- **Proof file:** same `style/admitted_gaps.rs` (a `preserved_candidates`
  table family).
- **Adjudications (decided, recorded here — the pins assert current values
  and the registry rows carry these classes and rationale):**
  - **near-white initial `color` → deliberate.** `[240/255;3, 1.0]` is the
    crate's dark-theme design-system default (definition site:
    `DEFAULT_TEXT_COLOR_RGBA` in `computed_color_group.rs`); CSS's
    `canvastext` default has no meaning for this product surface. Pinned
    permanently; re-reviewed if the design system changes.
  - **`line-height: normal` → `1.0 × font-size` → divergent.** Every browser
    uses ~1.2 (UA-dependent); D6 favors the browser answer. Pinned at
    current values, expected to move on-spec when a consumer need is named
    (reclassify deliberate only if the design system explicitly confirms
    tight rows are wanted).
  - **border widths initial `0` → deliberate.** CSS's `medium` initial only
    matters in combination with `border-style`; since `border-style` initial
    is `none`, undeclared borders are invisible in browsers too. Our
    width-gates-border model is observably identical for the undeclared
    case; the `border-style` admission would revisit.
- **Registry:** one row per candidate with the class and rationale above;
  the near-white and border rows are permanent (deliberate), the
  line-height row is expected to move (divergent).

**B5 data structures and algorithms (Phase 2 preview, line-height only —
the other two are pinned permanent).** Current state: `line-height: normal`
resolves to `None` in the font group, and the downstream default is
`font_size_px` (1.0×) at `text_layout/mod.rs:714-722`
(`effective_line_height_px`). The on-spec shape: `normal` resolves from
FONT METRICS, not a fixed factor — Blink's `FontMetrics::lineSpacing` and
Gecko's hhea-derived normal line height both compute
`ascent + descent + lineGap` (≈ 1.2 for most text faces). cosmic-text's
`FontSystem`/fontdb exposes the same metrics (`Metrics::new(font_size,
line_height)` vs the face's hhea table). The algorithm change is one
function: `normal` → query the resolved family's hhea metrics and compute
`line_height = ascent + descent (+ lineGap)` at the same site that today
returns `font_size_px`. The pin flips when it lands; the font-consumption
follower (B4) is a prerequisite in practice, since metric choice depends on
the resolved family.



## B6 — layout-impact properties

Not in the 68-property table: each is dropped+counted, inert, no crash.
Pin per property: the drop is counted AND the defined no-effect holds
(e.g. `z-index` has no effect on layering order, `sticky` behaves as
`static`, `display: grid` falls back to the initial display). Rows:
`z-index`, `float`, `clear`, `box-sizing`, `position: sticky` (value-side
drop), `display: grid` / `inline-grid` / `table` / `inline-table` /
`flow-root` / `contents` / `ruby` (value-side drops), `aspect-ratio`,
`object-fit`, `object-position`, `overflow-x` / `overflow-y` (also in B10),
`contain`, `content-visibility`, `break-inside`, `resize`, `scroll-behavior`,
`scroll-snap-*`, `overscroll-behavior`, `touch-action`, `will-change`.

**B6 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_properties.rs`
  (new; holds B6 and B11 rows as separate table families).
- **No production changes.** Each row is two-sided: the declaration is
  dropped+counted (`assert_decl_drop`) AND the defined no-effect holds. The
  no-effect assertion per family:
  - `z-index`: sibling paint order unchanged (staged paint plans keep DOM
    order; the layer order machinery at `composite/mod.rs` is unaffected).
  - `position: sticky`: layout equals `static` (the value drop leaves
    `position` at initial `Static`).
  - `display: grid`/`table`/`contents`/…: layout equals the initial display
    (the value drop leaves `DisplayKw::Block` for div).
  - `box-sizing`: sizing stays border-box (our only model — note in the pin
    header that `border-box` is the crate's single sizing model).
  - `aspect-ratio`, `object-fit`, `object-position`: replaced-element sizing
    and paint unchanged (img keeps intrinsic/placeholder behavior pinned in
    `layout/replaced_elements.rs`).
  - `float`, `clear`: flow participation unchanged.
  - `contain`, `content-visibility`, `break-inside`, `resize`,
    `scroll-behavior`, `scroll-snap-*`, `overscroll-behavior`, `touch-action`,
    `will-change`: layout/paint/scroll outcomes equal the no-declaration
    baseline (scroll behavior pinned by the existing scroll unit tests).
- **Registry:** `unsupported_*` rows with the D6 admission reference.

**B6 data structures and algorithms (Phase 2 preview, per property).**
Admission shape follows the redesign's standing rule: a property enters the
68-row table in `presentation_property.rs` (name, group, grammar — one
metadata row), a field appears in the owning group if one is missing
(`Copy` fields only — the S1 invariant that groups stay small, comparable,
and arena-free), and consumers read it visit-locally. Per family:
`z-index` → an order key on the layer (`composite/mod.rs` already maintains
`layer_order`; the algorithm is CSS 2.1 Appendix E stacking order —
tree-order sorted by z within stacking contexts; the field is an
`Option<i32>` on the box group). `position: sticky` → `PositionKw::Sticky`
plus the layout algorithm: the sticky constraint rectangle (nearest
scroll-port ancestor) clamps the static position at scroll time (CSS
Position 3 §sticky; the scroll machinery at `domains/visitor/scroll.rs`
already knows the container). `box-sizing` → `BoxSizingKw { BorderBox,
ContentBox }` on the box group; layout's width/height interpretation moves
inside one function (the sizing projection in `domains/layout/style.rs` —
border-box stays the default and the crate's current single model).
`aspect-ratio` → `Option<f32>` field; the CSS Sizing 4 preferred-ratio rule
constrains auto sizes in the replaced-element and absolute sizing paths.
`object-fit`/`object-position` → img paint algorithm (contain/cover/fill
letterboxing inside the content box, at `paint/mod.rs`'s image op).
`float`/`clear` → flow-layout additions (out-of-flow boxes, line
shortening, clear bands in `domains/layout/flow.rs`) — the largest item in
this bundle and a likely never-admit. `display: grid`/`table`/`contents` →
`grid` is a new solver (never-admit by default); `contents` is a
box-generation rule (suppress the own box, children join the parent's
formatting context) — medium, and the only display admission with a
plausible consumer need.



## B7 — color grammar

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `color: #abc` / `#abcd` | dropped+counted (hex6/8 only) | 3/4-digit hex | unimplemented |
| `color: rebeccapurple`, `color: gray` | dropped+counted (5 named only) | full named-color set | unimplemented |
| `color: currentColor` | dropped+counted | currentColor keyword | unimplemented |
| `color: rgb(255 0 0)`, `rgb(255 0 0 / 50%)` | **verify** space/slash handling; pin actual | modern rgb syntax | verify; likely unimplemented or partial |

**B7 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_colors.rs`
  (new; one small table family).
- **No production changes.** Drop-pins via `assert_decl_drop` plus a
  computed read of `as_color().color_rgba` / `as_visual().background_rgba`
  staying at the group initial.
- **Verify first:** the modern `rgb()` space-separated and slash-alpha forms
  against `parse_css_color_rgba` (`dom_attr_parse.rs:136+`) — the parser's
  header comment documents comma or space separated channels and alpha as
  0–1 or %, so parts may already be admitted; pin the actual accept/drop per
  form, on-spec where accepted, unimplemented where dropped.
- **Registry:** `unsupported_*` rows (plus on-spec rows for anything already
  admitted).

**B7 data structures and algorithms (Phase 2 preview).** Current state:
`DeclValue::Color([f32; 4])` is normalized at the declaration boundary by
`parse_css_color_rgba` (`dom_attr_parse.rs:136+`) — colors are typed from
birth, so consumers never re-parse (the redesign's P3 fix for the worst
corner: color used to ride as a `Custom(String)` heap key). Admissions are
pure grammar widenings at that one site: hex3/hex4 (digit-doubling, one
branch), the CSS named-color set (a static `&[(&str, [f32; 4])]` table —
the full ~148 names, sorted, binary search or a phf map), and modern
`rgb()` space/slash forms (the parser's channel splitter, likely
part-admitted already). `currentColor` is the one structural note: it is
not a color but a value REFERENCE — CSS computes it to the element's
`color` value — so it resolves at style time from the color group
(`color_rgba`); the declaration stores `Keyword(CurrentColor)` and the
resolver substitutes the element's own computed color (Servo does exactly
this at computed-value time; Blink keeps it symbolic to used time — our
style-time resolution matches Servo's shape and keeps groups fully
resolved, consistent with the A5/A6 doctrine).



## B8 — selectors & at-rules

An unsupported selector fails `parse_selector` → `DomError::InvalidSelector`
→ **the whole load fails** (fatal at load, unlike declaration drops).

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `div > span { }` | whole load `Err(InvalidSelector)` | child combinator | unimplemented |
| `div + span`, `div ~ span` | whole load Err | sibling combinators | unimplemented |
| `[type]`, `[type=button]` | whole load Err | attribute selectors | unimplemented |
| `div::before`, `p::first-line` | whole load Err | pseudo-elements | unimplemented |
| `:first-child`, `:last-child`, `:nth-child(2)`, `:not(.x)`, `:focus-within`, `:checked`, `:disabled` | whole load Err | more pseudo-classes | unimplemented |
| `.a, .b { }` (selector list) | whole load Err | selector lists | unimplemented |
| `@media (min-width: 100px) { .a { } }` | whole load Err | conditional rules | unimplemented |
| `@supports (display: flex) { }` | whole load Err | feature queries | unimplemented |
| `@keyframes spin { }`, `@font-face` | whole load Err | at-rules | unimplemented |
| stray text before a rule, stray `}`, missing closing `}` | load Err (`InvalidAttributeValue`, block structure) | CSS error recovery skips to next rule | adjudicate: G was scoped to declarations deliberately; pin current (fatal), note the boundary |

**B8 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_selectors.rs`
  (new).
- **No production changes.** Every selector row asserts the same two facts:
  `load_stylesheet(...)` returns `Err(DomError::InvalidSelector)` AND no
  rules from the sheet apply (a target matching the rule's selector presents
  the initial value — because the load failed atomically, nothing was
  inserted; verified in `style_runtime.rs::load_stylesheet`, the `?` on
  `compile_rule` propagates before insertion).
- **The fatal-shape is the accounting point:** each proof header notes
  loudly that ONE unsupported selector fails the ENTIRE sheet — consumers
  must strip unsupported selectors before load. The admission shape when a
  feature lands (per-feature): keep the fatal shape for the rest.
- **Stray-`}` / block-structure adjudication (decided, recorded here):** pin
  current whole-load-`Err(InvalidAttributeValue)` as **unimplemented**:
  statement-level error recovery (CSS 2.1 §4.1.7's malformed-statement
  rule, the statement-level sibling of G's declaration rule) is an
  admission-gated parser feature. The load error is loud (typed `Err` to
  the caller), so the current shape is acceptable until a consumer need is
  named; the natural admission shape mirrors G at statement level (skip the
  malformed statement, count it, continue).
- **`@media` note:** same unimplemented class, and the pin header carries
  the consumer-facing warning (one `@media` block fails the entire sheet).
- **Registry:** `unsupported_*` rows per family (combinators, attribute
  selectors, pseudo-classes, pseudo-elements, selector lists, at-rules,
  statement recovery).

**B8 data structures and algorithms (Phase 2 preview, per feature).**
Current state: `SimpleSelector` (`style/mod.rs`) is a flat per-compound
enum matched right-to-left with a parent-chain scan per preceding compound
(the A3 algorithm, shaped like Blink's `RuleSet` bucket + match and Servo's
`SelectorMap`); the right-most inverted indexes
(`id/class/tag/universal_index` in `style_runtime.rs`) make candidate
lookup O(indexed); the closed grammar rejects everything else at load. Per
feature: **selector lists (`,`)** — `parse_css_blocks` splits the selector
text at top-level commas; each member compiles to its own `CompiledRule`
SHARING one `DeclBlockKey` (the decl arena already dedups and shares
blocks by key — no storage change, one line of identity-hash thought:
identical (selector, block) pairs still dedup). **Combinators (`>`, `+`,
`~`)** — a `Combinator` enum between compounds; the matcher replaces the
chain scan for that compound with the exact relation (parent / previous
sibling / preceding siblings — all reachable from the node record's
sibling/parent keys, O(1) or O(siblings)). **Attribute selectors** — a
`SimpleSelector::Attr { name, value_matcher }` variant; matching reads
stored content attrs (`id`/`class` fast paths exist). **Structural
pseudo-classes** (`:first-child`, `:nth-child`, …) — computed on the visit
from sibling links (child index, child count); state pseudo-classes
(`:checked`, `:disabled`) read the widget state bits already on elements.
**`@media`** — the block parser learns at-rule parsing; a rule gains a
media condition (parsed `min/max-width/height` over our closed feature
set); `StyleRuntime` evaluates conditions at match time against the
viewport dimensions the composite stage already owns, and a viewport
resize bumps `revision` (the existing boundary-change seam) so
conditionality re-evaluates on the next update. **`@supports`** — trivial
with the closed property table (a feature is "supported" iff admitted).
**Statement recovery** — the block parser skips to the next `}` on a
malformed statement and counts it in the load report (G's declaration rule
lifted to statement level, with the same report-and-count contract).
**`@keyframes`** — deferred to F stage 3 (the animation tier, B9).



## B9 — cascade tiers

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `.a { width: 10px !important }` | `!important` token fails the value grammar → declaration dropped+counted; rule otherwise applies | important tier above normal author | unimplemented (F stage 1) |
| `style="width: 10px !important"` (inline) | dropped+counted | same | unimplemented |
| `@layer a { }` / layered origins | whole load Err (at-rule) | cascade layers | unimplemented (F stage 4) |
| `.a { --brand: #112233; width: var(--brand) }` | custom property dropped; `var(--brand)` dropped+counted | custom properties + var() | unimplemented |
| `animation: spin 1s` (declaration) | dropped+counted | animation tier | unimplemented (F stage 3) |
| behavior sheet vs consumer low-priority origin (the documented UA-sheet workaround) | works today (origin.priority ordering) — pin it | UA-sheet analog | on-spec (pin the workaround) |

**B9 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/cascade_tiers.rs`
  (new).
- **No production changes.**
- **`!important` rows:** pin the exact current mechanics — the
  `!important` token fails the value grammar, so the DECLARATION is
  dropped+counted while the rest of the rule still applies (`.a { width:
  10px !important; height: 5px }` → height applies, width is dropped).
  Assert both the drop count and the surviving declaration. Note in the
  header that F stage 1's design is already shaped (importance bits arrive
  as a parallel bit-vector, Servo `declarations_importance` shape — see the
  declaration-store design in git history and `decl_block.rs`'s header).
- **Custom properties / `var()`:** `--brand` drops at name lookup (counted);
  `width: var(--brand)` drops at value grammar (counted). Both rows assert
  the drops and the no-effect.
- **`@layer`:** whole-load `Err` (at-rule, same mechanics as B8).
- **Animation tier:** `animation: spin 1s` drops (name not in the table).
- **UA-sheet workaround (on-spec pin):** a stylesheet loaded at
  `StyleRuleOrigin::new("ua", -100)` (i) loses to a `priority: 0` author
  sheet at equal specificity and (ii) beats the behavior sheet (which folds
  at `i32::MIN`). This pins the documented "model a UA sheet as a
  low-priority origin" path so it cannot silently break.
- **Registry:** `unsupported_*` rows + the one on-spec workaround pin.

**B9 data structures and algorithms (Phase 2 preview, per tier).** Current
state: precedence is the `Precedence { origin_priority, inline,
specificity, order }` record (`decl_value.rs`) — widened at redesign time
exactly so these tiers are ADDITIVE (F's "adopt the structure, defer the
stages" verdict), with the behavior sheet folded at `i32::MIN` as the
UA-ish tier. **`!important` (F stage 1):** `DeclBlock` gains a parallel
importance bit-vector (the Servo `declarations_importance: SmallBitVec`
shape, already noted in `decl_block.rs`'s header); parsing strips the
`!important` suffix at declaration level (it is part of the declaration,
not the value — CSS Syntax §5); `Precedence` gains `important: bool` as its
FIRST field (important author beats all normal declarations including
normal inline; our tier order stays a projection of CSS Cascade §6.1:
transitions > important tiers > normal tiers). **Custom properties (F):**
the block gains a string-keyed custom-property map (Servo keeps a
`PropertyDeclarationIdSet` with a custom-name set — the same shape); `var()`
substitution happens at computed-value time with cycle detection and the
guaranteed-invalid concept (CSS Custom Properties §3; Blink/Servo both
substitute at computed-value time). **`@layer` (F stage 4):** a layer table
in `StyleRuntime` (first-seen layer order per origin) and a `layer: u16`
field inserted into `Precedence` per §6.1 (unlayered above layered; order
within layers by the table). **Animation tier (F stage 3):** keyframes
storage (`@keyframes` from B8), a per-property keyframed-interpolation
sampler sharing the transition machinery's `DeclValue` lerp table, and an
animation declaration tier in the winners fold between important and
normal (§6.1's order — the fold's `Precedence` already supports it). **Real
UA tier (F stage 2):** the `i32::MIN` behavior-sheet precedence is
formalized into a named tier field (the workaround pin in this bundle is
its interim guarantee).



## B10 — other shorthands & overflow axes

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `font: italic 12px serif` | dropped+counted | font shorthand | unimplemented |
| `background: #112233` | dropped+counted | background-color | unimplemented |
| `text-decoration: underline` | dropped+counted | unadmitted family | unimplemented |
| `all: initial` | dropped+counted | CSS-wide reset | unimplemented |
| `overflow: scroll auto` (two-token), `overflow-x`, `overflow-y` | dropped+counted | per-axis overflow | unimplemented |

**B10 scope and changes.**

- **Proof file:** same `style/unsupported_shorthands.rs` (a second table
  family next to B3's).
- **No production changes** — drop-pins plus initial-value computed reads,
  same shape as B3. The `overflow: scroll auto` row also asserts the
  single-token admitted form still works (guard against the pin
  over-reaching).
- **Registry:** `unsupported_*` rows.

**B10 data structures and algorithms (Phase 2 preview).** Identical in shape
to B3: new `Shorthand` variants + `property_shape` names + expansion fns in
`decl_parse.rs`. `overflow-x`/`overflow-y` additionally splits the box
group's single `overflow: OverflowKw` field into `[OverflowKw; 2]` (x, y) —
the ONE group-field change in the bundle, rippling to the clip/scroll
predicates (`creates_clip` = either axis ≠ Visible; `allows_vertical_scroll`
= the y axis) and their consumers (layer kind, scroll gating, flex auto-min
— all already centralized behind those two predicates, which is why the
split stays cheap). `font` maps to the font group's five longhands with the
CSS Fonts 4 shorthand rules (size/line-height/family required-ish, others
initial — the closed-subset projection of the full grammar).



## B11 — text/effects property tail

All dropped+counted, inert, no crash; one mechanical table. Rows:
`text-decoration*`, `text-transform`, `text-indent`, `text-shadow`,
`text-overflow`, `letter-spacing`, `word-spacing`, `line-clamp` /
`-webkit-line-clamp`, `hyphens`, `box-shadow`, `vertical-align`,
`writing-mode`, `unicode-bidi`, `visibility`, `cursor`, `user-select`,
`list-style*`, `filter`, `mix-blend-mode`, `backdrop-filter`, `clip-path`,
`mask*`, `image-rendering`, `border-collapse`, `border-spacing`,
`caption-side`, `empty-cells`, `table-layout`, `column-count`, `columns`,
`column-rule`, `orphans`, `widows`, `animation*` (declaration names).

**B11 scope and changes.**

- **Proof file:** same `style/unsupported_properties.rs` (the tail table
  family next to B6's).
- **No production changes.** ONE test driving a const table of ~30 property
  names — each row: a single-declaration stylesheet asserts `applied == 0,
  dropped == 1` through `assert_decl_drop` (the enumerable-coverage pattern
  the README conventions ask for). `text-decoration` and `box-shadow` get
  one extra no-effect assertion each (paint plan unchanged) since they are
  the two most popular rows in the tail.
- **Registry:** the family registers as one `unsupported_*` entry naming the
  whole tail table rather than 30 individual rows (the registry stays
  readable; the table itself is the enumeration).

**B11 data structures and algorithms (Phase 2 preview, by popularity).**
`text-decoration` → a text-group field (`TextDecorationKw` line set +
color) with paint emitting the decoration line under/over text runs (the
text paint path already walks runs with their baselines — decoration is a
rect per run segment). `box-shadow` → a bounded shadow list on the visual
group (`ArrayVec<ShadowSpec, 4>` — the damage-rect/transition-longhand
bounded-list precedent: fixed capacity, over-capacity rejected at
admission, revisitable via D6) with paint emitting blurred/offset rects
before the box (blur via the existing rect-op corner machinery or a new op
kind). `text-overflow: ellipsis` → a paint-time clip + ellipsis run
substitution at the line box (cosmic-text provides the ellipsis glyph; the
line-layout walk already knows overflow). Everything else in the tail
admits one property-table row and, where it has no engine meaning yet
(`cursor`, `will-change`, `visibility: collapse`-only rows), stays a
stored computed value until a consumer names the effect — storage is
cheap; behavior is admission-gated per effect.



## B12 — robustness

Every scenario asserts the defined outcome AND returns without panic.

- **Empty/whitespace**: empty stylesheet, whitespace-only, `.a { }`,
  `.a { ; ; }`, `style=""`, `style=";"`.
- **Block-structure abuse**: missing `}`, stray `}`, `}}`, `{` with no `}`
  (fatal load errors are the defined outcome — pin, no crash).
- **Comments**: `/* unterminated`, `/*a*/ width: 1px`, `// line` (verify
  handling), comments inside values.
- **Volume**: 10k-rule stylesheet (loads, no hang), one rule with 200
  declarations, a 64-compound descendant selector (no stack blow).
- **Extreme values**: `font-size: 100000px` render, `margin-left: -3.4e38px`,
  `opacity: 1.0000001`, plus B1's numeric rows' render-side proofs.
- **Typed-writer abuse**: `style_mut(key).set("", "")`, `set("width", "")`,
  `set_all` with 200 declarations, `set` on removed-node key
  (`InvalidNodeKey`), set on document/fragment/text keys
  (`InvalidAttributeTarget`).
- **Encoding**: UTF-8 BOM at stylesheet start, non-ASCII strings, NUL bytes
  in a declaration.
- **`font-size: 0` / `line-height: 0` render** (the crash-adjacent B1 rows —
  the render-side proofs live here).

**B12 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/robustness.rs` (new,
  at the suite root — it is cross-cutting, not style-domain-only).
- **Volume:** 10,000 rules in one stylesheet load asserts `Ok` with
  `rules_inserted == 10_000` and one match against the last rule (no hang —
  keep the assertion outcome-only, no wall-clock pins); one rule with 200
  declarations asserts the report counts and a sampled computed value; a
  64-compound descendant selector over a 64-deep div nest matches the leaf
  (the parent-chain scan must not blow the stack).
- **Block-structure abuse:** the fatal load `Err` outcomes are the defined
  behavior — pin them (they overlap B8's adjudicated rows deliberately; the
  robustness file pins the no-crash side, B8 pins the class).
- **Comments:** `strip_block_comments` (`style/mod.rs:274-300`) is the
  owner; pin unterminated comments (defined outcome — verify),
  comment-adjacent declarations (work), and `// line` at both value level
  (drops with the declaration) and statement level (selector error —
  verify and pin the actual).
- **Encoding:** BOM `\u{FEFF}` at stylesheet start (verify: trimmed or
  selector error — pin actual), non-ASCII in strings (works — `font-family`
  with a CJK name interns fine), NUL bytes in a declaration (defined
  outcome, no crash).
- **Typed-writer abuse:** extends `style_mut.rs`'s existing error-path
  tests into the proof suite (empty name/value, removed-node key →
  `InvalidNodeKey`, non-element targets → `InvalidAttributeTarget`,
  `set_all` with 200 declarations → bounded report).
- **Crash-hardening rule (reiterated):** any panic discovered is a
  boundary-policy bug — the fix lands in the same change (typed error or
  drop) with the pin asserting the hardened behavior, and this document
  records the fix below.
- **Registry:** the family registers as one robustness entry naming the
  file; individual hardening fixes get their own registry notes as they
  happen.

### Hardening fixes landed (running record)

_None yet._

**B12 data structures and algorithms.** No Phase 2 — this bundle is the
permanent safety net, not an admission track. The scenarios stress the
system's existing bounded structures exactly where they are designed to
hold: the fixed-capacity history ring and enum-indexed counters (metrics
doctrine — no allocation under load), the `SlotMap` arenas (10k-rule
volume exercises generational-key density, not pointer storms), the
`SmallVec`/`ArrayVec` bounded lists (200-declaration blocks), the
parent-chain selector scan (64-deep nesting — iterative, not recursive, by
construction), and the typed-error boundary (every malformed input returns
`DomError` or drops with a count — the two sanctioned outcomes, never a
panic). A failure of any of these is a boundary-policy bug with a fix in
the same change, recorded above.



## Open verifications (resolve at pin time; correct the doc in the same change)

- `rgb()` space-separated / slash-alpha handling in the current color parser.
- `text-align: justified` and the `pre*`/`break-spaces` white-space modes:
  real consumption in text layout or keyword-only (justify is verified
  CONSUMED — `text_render.rs:106`; the white-space modes are open).
- `border-radius` paint clamp behavior on oversized radii.
- Whether `font-size: 0` and negative sizes can reach a panic in
  cosmic-text metrics (then harden: grammar rejects `< 0`, and `0` renders
  nothing without crashing).

Resolved this session: the `pointer-events` channel (content-side
`Custom("pointer-events")`, `none`/`auto`/inherit at `hit_test.rs:332-336`
— see B4) and the `order` clamp target (saturating i64→i32 at
`project.rs:444-445` — see B1).
