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


## Open verifications (resolve at pin time; correct the doc in the same change)

- `rgb()` space-separated / slash-alpha handling in the current color parser.
- `pointer-events`: the actual attribute channel and consumed values
  (hit_test.rs reads something — name it).
- `text-align: justified` and the `pre*`/`break-spaces` white-space modes:
  real consumption in text layout or keyword-only.
- `order` clamp target (i64 → i32 saturate vs wrap).
- `border-radius` paint clamp behavior on oversized radii.
- Whether `font-size: 0` and negative sizes can reach a panic in
  cosmic-text metrics (then harden: grammar rejects `< 0`, and `0` renders
  nothing without crashing).
