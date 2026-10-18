# PLAN.md — style-system coverage accounting (2026-10-18)

The declaration-store style redesign (proposals A+B+C on substrate S1) is
LANDED at v0.6.0 (see `MIGRATION.md` for the API mapping and the git history
for the redesign plan it replaced). This document is the **coverage
accounting plan**: an enumeration of every spec area the engine has
deliberately set aside — unsupported value forms, unimplemented shorthands
and properties, absent cascade tiers, selector-subset gaps, admitted-but-
unconsumed behaviors, and preserved pre-redesign quirks — and the working
discipline for pinning their CURRENT behavior with running proofs.

The purpose is accounting, not redesign:

1. **Know what we do.** Every set-aside area gets a running proof that pins
   the current observable behavior (usually: the declaration is dropped and
   counted, the write is a typed error, or the feature is inert).
2. **Nothing may crash us.** Every pinned scenario also proves the input is
   handled without a panic. A discovered panic on boundary data is a repo-rule
   bug ("Panic and boundary policy", AGENTS.md) and is hardened in the same
   change as its pin (typed error or drop — never panic), with the fix noted
   in the pin and in this document.
3. **The registry stays honest.** Each pin is classified divergent /
   unimplemented / deliberate / on-spec per the conventions below and
   registered in `tests/layout_proofs/README.md` in the same change. No
   production behavior changes except crash-hardening and explicitly-adjudicated
   divergence fixes (each its own call, recorded here).

## Conventions (read before writing any pin)

- **Pin classes** (the layout_proofs README's registry classes, extended):
  - **divergent** (`off_spec_*`) — current behavior contradicts the spec
    *within or beside the admitted vocabulary*; pinned at current values,
    expected to move on-spec.
  - **unimplemented** (`unsupported_*`) — a spec area the vocabulary does not
    admit (D6 policy-gated); the pin asserts the gate behavior (declaration
    dropped + counted, or selector/at-rule load error) and no crash.
  - **deliberate** — a crate-owned value replacing a spec default that has no
    meaning for our world; permanent, re-reviewed on spec-area revisit.
  - **on-spec** — the accounting found the current behavior already correct;
    pin it so it cannot silently move.
  - **robustness** (`robustness_*`) — crash-resistance pins over adversarial
    inputs; class is orthogonal (a robustness pin can sit over any of the
    classes above).
- **Every pin states four things** in its header (the off-spec comment
  convention): the spec claim + citation, CURRENT (asserted), SPEC-CORRECT,
  ROOT CAUSE. `unsupported_*` pins replace ROOT CAUSE with the admission
  policy reference (D6 in `MIGRATION.md`).
- **Proof shape**: table-driven per the README conventions; scenario through
  public `RootDom` APIs; assert the observable (computed groups via
  `root.computed_style(...)`, load reports, metric counters like
  `declaration_drop_count` / `StyleDeclarationDropped`, layout rects, staged
  paint ops). A passing test is the no-crash proof (a panic fails the test).
- **Honesty rule for expectations**: the "expected current" column in this
  document is my best verified knowledge at writing time. The pinning task
  verifies each row; where reality differs, the PIN records reality and the
  same change corrects this document's row.
- **No production behavior change** except the crash-hardening rule above
  and any divergence fix explicitly adjudicated in this document. All gates
  green per landing: fmt, clippy, workspace tests, `cargo xtask lint`.

## The new proof files (all under `crates/ui-dom/tests/layout_proofs/`)

| File | Areas | Classes |
| --- | --- | --- |
| `unsupported_values.rs` | A (units, numbers, colors, CSS-wide keywords) | mostly unimplemented, some divergent/on-spec |
| `unsupported_shorthands.rs` | B (unimplemented shorthands) | unimplemented |
| `unsupported_properties.rs` | C (property-table absences) | unimplemented |
| `unsupported_selectors.rs` | D (selector subset, at-rules) | unimplemented, some on-spec |
| `cascade_tiers.rs` | E (`!important`, custom properties, `var()`, UA sheet, animations, layers) | unimplemented |
| `admitted_gaps.rs` | F (admitted-but-unconsumed/degraded) + G (preserved candidates) | divergent/deliberate/on-spec per row |
| `robustness.rs` | H (crash-resistance) | robustness |

Each file registers in `tests/layout_proofs/mod.rs`; the README's registry
and spec-coverage map grow per landing batch.

## Area A — unsupported value forms (`unsupported_values.rs`)

Current length grammar: `unset | 0 | <px> | <em>`. Current color grammar:
`transparent`, hex6/hex8, `rgb()/rgba()`, named `white|black|red|green|blue`.
CSS-wide keywords: only `unset` (and only where the grammar says so).

| Input (representative) | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `width: 50%` | dropped+counted; width stays initial | resolve against containing block | unimplemented |
| `margin-left: 10%`, `font-size: 120%`, `line-height: 150%` | dropped+counted | percentages | unimplemented |
| `width: calc(100% - 10px)`, `calc(1px + 1px)`, `min()/max()/clamp()` | dropped+counted | math functions | unimplemented |
| `width: 100vw`, `height: 50vh`, `10vmin`, `10vmax` | dropped+counted | viewport units | unimplemented |
| `font-size: 2rem` | dropped+counted | root em | unimplemented |
| `1ex`, `2ch`, `1lh`, `1rlh` | dropped+counted | relative units | unimplemented |
| `width: auto`, `margin-top: auto` | dropped+counted (no width set; observably ≈ initial for width) | `auto` keyword | unimplemented |
| `width: min-content` / `max-content` / `fit-content` | dropped+counted | intrinsic size keywords | unimplemented |
| `width: 10` (unitless non-zero) | dropped+counted | **CSS also rejects** | on-spec (pin to keep) |
| `width: 1e400px`, `NaNpx`, `infpx`, `-infpx` | dropped+counted (non-finite rejected) | finite values only; CSS would clamp/parse per syntax | on-spec-ish; robustness |
| `padding-left: -10px` | **accepted** (no sign check — verified `parse_authored_length`) → negative inset in layout | **illegal, CSS rejects** (padding is non-negative) | **divergent — fix candidate** |
| `width: -10px`, `height: -1px` | **accepted** | illegal (non-negative) | **divergent — fix candidate** |
| `border-top-width: -2px`, `border-radius: -4px` (via longhand), `gap: -4px`, `outline-width: -2px` | **accepted** | illegal | **divergent — fix candidate** |
| `outline-offset: -2px` | accepted | **legal** (negative allowed) | on-spec (pin to keep) |
| `margin-left: -10px`, `top: -5px` | accepted | legal | on-spec (already covered? keep) |
| `font-size: -10px` | **accepted** → negative font size into text layout | illegal | **divergent + crash-adjacent — fix candidate (reject at grammar)** |
| `font-size: 0` (bare) and `0px` | admitted → 0px font into text layout | legal in CSS | robustness (must not crash text shaping/intrinsics) |
| `line-height: 0` | admitted → 1px floor (verified `resolve_line_height_px`) | legal | on-spec (pin the floor) |
| `color: #abc` / `#abcd` | dropped+counted (hex6/8 only) | 3/4-digit hex | unimplemented |
| `color: rebeccapurple`, `color: gray`, `color: currentColor` | dropped+counted | named-color set, currentColor | unimplemented |
| `color: rgb(255 0 0)`, `rgb(255 0 0 / 50%)` | verify current `rgb()` space/slash handling; pin actual | modern rgb syntax | verify; likely unimplemented or partial |
| `width: inherit`, `width: initial`, `width: revert`, `width: revert-layer` | dropped+counted (only `unset` admitted) | CSS-wide keywords | unimplemented |
| `font-size: unset` | dropped+counted (grammar is `LengthNoUnset`) | `unset` is CSS-wide (≈ inherit here) | unimplemented |
| `opacity: 1.5`, `opacity: -0.5` | dropped+counted (range 0..=1) | out-of-range invalid (CSS clamps at computed time — strictness choice, note it) | unimplemented-ish; pin |
| `flex-grow: -1` | dropped+counted | non-negative | on-spec (pin to keep) |
| `order: 1.5` | dropped+counted | integer | on-spec (pin to keep) |
| `order: 99999999999999999999` (beyond i64) | dropped+counted | huge values valid in CSS | unimplemented |
| `order: 9223372036854775807` (i64::MAX) | accepted → clamped to i32 (verify clamp target) | valid | robustness (no crash on clamp) |
| `transform: translate(999999px, -999999px)`, `scale(2)`, `rotate(45deg)`, multi-function chains | scale/rotate/chains dropped; extreme translate accepted (no clamp) | typed transforms | unimplemented + robustness |
| `border-radius: 10px / 20px` | dropped+counted | elliptical radii | unimplemented |
| `transition-property: width, unknown-thing` (longhand list w/ unknown item) | dropped+counted (whole declaration) | unknown items legal, just never trigger | unimplemented-ish; pin |
| `transition-duration: 120ms, 1s, 2s, 3s, 4s` (5 items > capacity 4) | dropped+counted | unbounded lists in CSS | unimplemented (documented capacity) |

## Area B — unimplemented shorthands (`unsupported_shorthands.rs`)

Today each is a dropped declaration (name not in the property table).
All are admission candidates (D6); margin/padding/border are the named
families the redesign already noted.

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `margin: 8px`, `margin: 1px 2px 3px 4px` | dropped+counted | margin ×4 | unimplemented |
| `padding: 8px` … | dropped+counted | padding ×4 | unimplemented |
| `border: 1px`, `border-width: 1px 2px` | dropped+counted | border widths ×4 (`border` full shorthand's style/color components unadmitted) | unimplemented |
| `inset: 0`, `inset: 1px 2px` | dropped+counted | top/right/bottom/left | unimplemented |
| `outline: 2px` | dropped+counted | outline-width (style/color unadmitted) | unimplemented |
| `font: italic 12px serif` | dropped+counted | font shorthand | unimplemented |
| `background: #112233` | dropped+counted | background-color | unimplemented |
| `text-decoration: underline` | dropped+counted | unadmitted family | unimplemented |
| `all: initial` | dropped+counted | CSS-wide reset | unimplemented |
| `overflow: scroll auto` (two-token) / `overflow-x` / `overflow-y` | dropped+counted | per-axis overflow | unimplemented |

## Area C — property-table absences (`unsupported_properties.rs`)

Every name below is not in the 68-property table: the declaration is
dropped+counted, the feature inert, no crash. One row per property, pinned
via a stylesheet + computed read (property absent from the node's computed
groups where the group has a field for it — otherwise assert the defined
no-effect, e.g. layout unchanged). Rows: `z-index`, `float`, `clear`,
`box-sizing`, `position: sticky` (value-side: `position` itself is admitted
but `sticky` is a dropped value), `display: grid` / `inline-grid` / `table` /
`inline-table` / `flow-root` / `contents` / `ruby` (value-side drops),
`aspect-ratio`, `object-fit`, `object-position`, `vertical-align`,
`text-decoration*`, `text-transform`, `letter-spacing`, `word-spacing`,
`line-clamp` / `-webkit-line-clamp`, `text-overflow`, `writing-mode`,
`direction` is admitted (no row), `unicode-bidi`, `visibility`, `cursor`,
`user-select`, `list-style*`, `border-collapse`, `border-spacing`,
`caption-side`, `empty-cells`, `table-layout`, `resize`, `scroll-behavior`,
`scroll-snap*`, `overscroll-behavior`, `touch-action`, `will-change`,
`contain`, `content-visibility`, `break-inside`, `orphans`, `widows`,
`image-rendering`, `filter`, `mix-blend-mode`, `isolation` is admitted
(no row), `backdrop-filter`, `clip-path`, `mask*`, `animation*`,
`transition` (admitted — no row), `grid*`, `column-count`, `columns`,
`column-rule`, `hyphens`, `text-indent`, `text-shadow`, `box-shadow`,
`pointer-events` (**verify first**: hit_test.rs reads a `pointer-events`
channel — pin the actual channel and its values, whatever it is today;
class accordingly).

## Area D — selector subset and at-rules (`unsupported_selectors.rs`)

Current: the closed subset (`#id`, `.class` multi, `tag`, `tag.class`, `*`,
descendant, `:hover|:active|:focus|:focus-visible`). An unsupported selector
fails `parse_selector` → `DomError::InvalidSelector` → **the whole load
fails** (fatal at load time, unlike declaration drops).

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `div > span { }` | whole load `Err(InvalidSelector)` | child combinator | unimplemented |
| `div + span`, `div ~ span` | whole load Err | sibling combinators | unimplemented |
| `[type]`, `[type=button]` | whole load Err | attribute selectors | unimplemented |
| `div::before`, `p::first-line` | whole load Err | pseudo-elements | unimplemented |
| `:first-child`, `:last-child`, `:nth-child(2)`, `:not(.x)`, `:focus-within`, `:checked`, `:disabled` | whole load Err | more pseudo-classes | unimplemented |
| `.a, .b { }` (selector list) | whole load Err | selector lists | unimplemented |
| `@media (min-width: 100px) { .a { } }` | whole load Err (block parser sees a selector starting with `@`) | conditional rules | unimplemented |
| `@supports (display: flex) { }` | whole load Err | feature queries | unimplemented |
| `@keyframes spin { }` / `@font-face` | whole load Err | at-rules | unimplemented |
| stray text before a rule, stray `}` in a stylesheet, missing closing `}` | load Err (`InvalidAttributeValue` on block structure) | CSS error recovery skips to the next rule | **divergent vs CSS 2.1 §4.1.7's statement recovery? — adjudicate: G was scoped to declarations deliberately; pin current (fatal) as unimplemented/robustness, note the boundary** |

## Area E — cascade tiers (`cascade_tiers.rs`)

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `.a { width: 10px !important }` | the `!important` token fails the value grammar → declaration dropped+counted; rule otherwise applies | important declarations form a tier above normal author | unimplemented (F stage 1) |
| `style="width: 10px !important"` (inline) | dropped+counted | same | unimplemented |
| `@layer a { }` / layered origins | whole load Err (at-rule) | cascade layers | unimplemented (F stage 4) |
| `.a { --brand: #112233; width: var(--brand) }` | custom property dropped; `var(--brand)` value dropped+counted | custom properties + var() | unimplemented |
| animation shorthand/keyframes in declarations (`animation: spin 1s`) | dropped+counted | animation tier | unimplemented (F stage 3) |
| behavior sheet vs a consumer-loaded low-priority origin (the documented "model a UA sheet as a low-priority origin" path) | works today (origin.priority ordering) — pin it | UA-sheet analog | on-spec (pin the documented workaround) |
| transitions vs `!important` fantasy (tier-1 claim) | n/a — document the overlay's tier-1 equivalence is already pinned (transitions.rs) | — | on-spec (already pinned; reference only) |

## Area F — admitted-but-unconsumed or degraded (`admitted_gaps.rs`)

Admitted grammars whose computed values are ignored, approximated, or
quirky downstream. Each row needs a verify-first at pin time.

| Behavior | Expected current (verify) | Spec-correct | Class |
| --- | --- | --- | --- |
| `font-family` computed but ignored by text layout (hardcoded UiSans) | text renders UiSans regardless | consume the computed family | **divergent (known step-5 follower)** — pin |
| `font-weight` / `font-style` computed but ignored by text layout | rendered regular/upright | consume weight/style | divergent (same follower family) — pin |
| `text-align: justify`/`justified` consumption in text layout | verify: justified lines or start-like fallback | justified text | verify; pin actual |
| `font-style: oblique` | dropped+counted (closed set is normal/italic) | oblique admitted in CSS | unimplemented |
| `white-space: pre` / `pre-line` / `break-spaces` consumption | admitted keywords; verify each maps to a real text-layout mode | full white-space modes | verify per keyword; pin actual |
| `transform` transitions (`transition: transform 100ms`) | **no transition schedules** (value jumps; v0.5 had a delayed step) | transform interpolates | **divergent — documented gap (MIGRATION.md)**; pin |
| range control with authored `border-radius: 0px` | falls to pill default (flattened presence) | authored value wins over control default | **divergent (documented)**; pin; the fix is the behavior-sheet tier (F) |
| `dir` content attr vs `direction` property on the same node | `direction` wins (documented) | HTML/CSS interaction | on-spec (pin to keep) |
| `pointer-events` channel (see Area C) | verify | — | place here or Area C |

## Area G — preserved pre-redesign candidates (`admitted_gaps.rs`, same file)

These were surfaced by the redesign and deliberately preserved (D8) but
never pinned. Each row needs a divergent-vs-deliberate adjudication at pin
time (record the call here and in the registry):

| Behavior | Current | Spec default | Notes |
| --- | --- | --- | --- |
| initial `color` when nothing is declared | near-white `[240/255;3, 1.0]` (design-system default) | `canvastext` (black) | likely **deliberate** (dark-theme crate default) — adjudicate |
| `line-height: normal` | `1.0 × font-size` | UA-dependent, typically ~1.2 | divergent or deliberate — adjudicate |
| border widths when undeclared | `0px` | `medium` | divergent or deliberate (borders opt-in) — adjudicate |
| 16:9 placeholder ratio for never-loaded images | already **deliberate**, registered | 300×150 default object size | already pinned; no work |

## Area H — robustness (`robustness.rs`)

Every scenario asserts the defined outcome AND returns without panic.
A discovered panic is a boundary-policy bug: harden in the same change.

- **Empty and whitespace inputs**: empty stylesheet, whitespace-only, a rule
  with an empty declaration block `.a { }`, `.a { ; ; }`, `style=""`,
  `style=";"`.
- **Block-structure abuse**: missing closing `}`, stray `}`, `}}`, `{` with
  no `}` (fatal load errors are the defined outcome — pin them, no crash).
- **Comment handling**: `/* unterminated`, `/*a*/ width: 1px`, `// line`
  (not CSS — how is it handled today? verify; pin actual), comments inside
  values.
- **Volume**: 10k-rule stylesheet (must load; pin time budget loosely — no
  hang), one rule with 200 declarations (must load), a selector with a
  64-compound descendant chain (match must not blow the stack).
- **Extreme values**: `width: 3.4028235e38px` (f32::MAX), `width: 1.4e-45px`
  (subnormal), `font-size: 100000px` text render, `margin-left: -3.4e38px`,
  `order: i64::MIN/i64::MAX` (clamp path), `opacity: 1.0000001`,
  `border-radius: 10000px` on a 10px box (paint clamps? verify).
- **Typed-writer abuse**: `style_mut(key).set("", "")`, `set("width", "")`,
  `set_all` with 200 declarations, `set("width", "10px")` on a removed-node
  key (`InvalidNodeKey`), set on a document/fragment/text key
  (`InvalidAttributeTarget`).
- **Encoding**: UTF-8 BOM at stylesheet start, non-ASCII in strings,
  `\0` bytes (NUL) in a declaration.
- **font-size: 0 / line-height: 0 render** (from Area A, the crash-adjacent
  rows — the render-side proof lives here).

## Landing batches (each its own green landing)

1. **README conventions + Area A** (values): the registry conventions grow
   the `unsupported_*` naming and robustness class note in the same change.
2. **Area B** (shorthands) — small.
3. **Area C** (properties) — mechanical table.
4. **Area D** (selectors/at-rules) — includes the stray-`}` adjudication.
5. **Area E** (cascade tiers).
6. **Area F+G** (admitted gaps + the three preserved candidates with their
   adjudications).
7. **Area H** (robustness) — crash-hardening fixes land with their pins.

Order is cheapest-first and dependency-free; any batch can be reordered.
Each batch updates this document's rows to match verified reality and grows
the README registry in the same change.

## Open verifications (resolve at pin time, correct the doc in the same change)

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
