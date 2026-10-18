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

## B3 — box shorthands

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `margin: 8px`, `margin: 1px 2px 3px 4px` | dropped+counted | margin ×4 | unimplemented |
| `padding: 8px` … | dropped+counted | padding ×4 | unimplemented |
| `border: 1px`, `border-width: 1px 2px` | dropped+counted | border widths ×4 (style/color components unadmitted) | unimplemented |
| `inset: 0`, `inset: 1px 2px` | dropped+counted | top/right/bottom/left | unimplemented |
| `outline: 2px` | dropped+counted | outline-width (style/color unadmitted) | unimplemented |

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

## B5 — preserved pre-redesign candidates

Preserved per D8, never pinned. Divergent-vs-deliberate adjudication at pin
time, recorded here and in the registry:

| Behavior | Current | Spec default | Notes |
| --- | --- | --- | --- |
| initial `color` when nothing declared | near-white `[240/255;3, 1.0]` (design-system default) | `canvastext` (black) | likely **deliberate** (dark-theme crate) — adjudicate |
| `line-height: normal` | `1.0 × font-size` | UA-dependent, ~1.2 | divergent or deliberate — adjudicate |
| border widths when undeclared | `0px` | `medium` | divergent or deliberate (borders opt-in) — adjudicate |
| 16:9 placeholder ratio (never-loaded images) | already **deliberate**, registered | 300×150 default object size | already pinned; no work |

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

## B7 — color grammar

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `color: #abc` / `#abcd` | dropped+counted (hex6/8 only) | 3/4-digit hex | unimplemented |
| `color: rebeccapurple`, `color: gray` | dropped+counted (5 named only) | full named-color set | unimplemented |
| `color: currentColor` | dropped+counted | currentColor keyword | unimplemented |
| `color: rgb(255 0 0)`, `rgb(255 0 0 / 50%)` | **verify** space/slash handling; pin actual | modern rgb syntax | verify; likely unimplemented or partial |

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

## B9 — cascade tiers

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `.a { width: 10px !important }` | `!important` token fails the value grammar → declaration dropped+counted; rule otherwise applies | important tier above normal author | unimplemented (F stage 1) |
| `style="width: 10px !important"` (inline) | dropped+counted | same | unimplemented |
| `@layer a { }` / layered origins | whole load Err (at-rule) | cascade layers | unimplemented (F stage 4) |
| `.a { --brand: #112233; width: var(--brand) }` | custom property dropped; `var(--brand)` dropped+counted | custom properties + var() | unimplemented |
| `animation: spin 1s` (declaration) | dropped+counted | animation tier | unimplemented (F stage 3) |
| behavior sheet vs consumer low-priority origin (the documented UA-sheet workaround) | works today (origin.priority ordering) — pin it | UA-sheet analog | on-spec (pin the workaround) |

## B10 — other shorthands & overflow axes

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `font: italic 12px serif` | dropped+counted | font shorthand | unimplemented |
| `background: #112233` | dropped+counted | background-color | unimplemented |
| `text-decoration: underline` | dropped+counted | unadmitted family | unimplemented |
| `all: initial` | dropped+counted | CSS-wide reset | unimplemented |
| `overflow: scroll auto` (two-token), `overflow-x`, `overflow-y` | dropped+counted | per-axis overflow | unimplemented |

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
