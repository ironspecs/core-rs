# MIGRATION.md — ui-dom v0.6.0: the declaration-store style system

v0.6.0 lands the style-system redesign specified in `PLAN.md` (proposals
A+B+C on substrate S1, decision D5). It is a **hard break** for every
consumer of the old direct styling writes. This document is the conversion
guide: the API mapping, the two enforcement paths, the parse-tightening
table, and the snapshot-contract notes.

The one-sentence summary: **presentation properties are no longer node
attributes.** Consumers write *declarations* into a per-element declaration
store (`style_mut` or the `style` attribute); the style pass resolves them
into typed computed groups once per rendering update, exactly like a browser
(css-style-attr §4, CSS Cascade 5 §4).

## The API mapping

### Writing styles

| v0.5 (removed) | v0.6 |
| --- | --- |
| `root.set_attr(key, DomAttribute::Width, Some(...))` | **does not compile** — the presentation variants are gone from `DomAttribute` |
| `root.set_attr_str(key, "width", "10px")` | `root.style_mut(key).set("width", "10px")?` |
| `root.set_attr_str(key, "style", "width: 10px")` | unchanged — still accepted; now routes into the inline declaration store with CSS error handling (below) |
| `root.set_attr_str(key, "gap", "12px")` | `root.style_mut(key).set("gap", "12px")?` (shorthands expand at parse) |
| removing a property: `set_attr(key, Width, None)` | `root.style_mut(key).remove("width")?` |
| wholesale inline cssText | `root.style_mut(key).set_all("width: 10px; height: 40px")? -> DeclDropReport` |
| `root.set_attr_str(key, "color", "#112233")` | `root.style_mut(key).set("color", "#112233")?` (custom color names are closed `PresentationProperty` variants now) |
| `<img>` `width`/`height` attr writes | `style_mut(key).set("width"/"height", "Npx")` — CSS sizing through layout; the intrinsic system keeps loaded-dimension and placeholder behavior |
| behavior-sheet authoring: `hover_style(smallvec![(DomAttribute::Custom("background-color".into()), DomAttributeValue::Color(..))])` | `hover_style(smallvec![Declaration { property: PresentationProperty::BackgroundColor, value: DeclValue::Color(..) }])` |

**The two enforcement paths.** The typed `set_attr` loses its presentation
variants at compile time (the compile break converts most call sites for
you). The string-keyed `set_attr_str(key, "width", ...)` and
`set_attr(key, DomAttribute::Custom("width"), ...)` fail fast at runtime
with `DomError::PresentationRequiresStyleMut` — loud, never a silent route,
never a panic. `Custom(name)` with a *non*-presentation name stays the
content-data escape hatch and stores as before.

**The two `style_mut` contracts are deliberately different.** `set` is
strict: one malformed or unadmitted declaration is a typed
`DomError::InvalidAttributeValue`. `set_all` follows CSS 2.1 §4.1.7 /
css-style-attr §3 (drop-and-continue): malformed declarations are dropped,
parsing continues, and the returned `DeclDropReport { applied, dropped }`
makes every drop observable. The `style` attribute write path follows
`set_all` semantics; its drops are counted on
`RootDomMetricName::StyleDeclarationDropped`.

**Timing.** There is exactly one clock: a `style_mut` write marks the
element style-dirty and the next `RootDom::update` resolves it in the style
phase, before layout and paint. The old immediate-apply semantics of direct
attr writes are gone — that was the bug (PLAN.md P1/P2).

### Reading styles

| v0.5 (removed) | v0.6 |
| --- | --- |
| `node.presented_attr(&DomAttribute::Width)` | `root.computed_style(key).expect(..).as_box().width` (`Option<f32>`, px-resolved) |
| `node.presented_attr(&DomAttribute::Custom("color"))` | `root.computed_style(key).expect(..).as_color().color_rgba` |
| `root.compute_style_map(key, parent_map)` | `root.compute_style_map(key)` → `BTreeMap<PresentationProperty, DomAttributeValue>` — an on-demand **debug-priced** recalc (the `getComputedStyle` freedom); do not call per-frame |
| `node.canonical_style_snapshot()` | `root.canonical_style_snapshot(key)` — serializes the node's inline declarations as `(String, DomAttributeValue)` pairs (arena keys are never serialized) |
| `node.canonical_snapshot()` | `root.canonical_snapshot(key)` |
| layout/paint/text internals reading attrs | unchanged for consumers — the crate reads the computed groups visit-locally |

### Stylesheets

- `RootDomContext::load_stylesheet(origin, css)` now returns
  `Result<StylesheetLoadReport, DomError>`:
  `{ rules_inserted, declarations_applied, declarations_dropped }`.
  Declaration parsing is drop-and-continue (CSS 2.1 §4.1.7): one malformed
  declaration no longer rejects the sheet. Selector syntax errors and
  malformed block structure still fail the load.
- `RootDomContext::declaration_drop_count()` — monotonic drop counter.
  `RootDomContext::declaration_block_count()` — live declaration blocks
  (level accessor).
- The `transition` **shorthand is now admitted** in stylesheets and expands
  to its four longhands at parse (CSS Transitions 1 §2).
- All shorthands expand at declaration-parse time: `border-radius`,
  `border-color`, `gap`, `flex`, `flex-flow`, `place-content`,
  `place-items`, `place-self`, `transition`, plus the four `border-<side>`
  width aliases (width-only in this vocabulary). Longhands always win over
  shorthands by cascade order (CSS Cascade 5 §3).
- `StyleRule` and `RootDom::apply_styles` are removed from the public API.

### Transitions

- `DomTransition` / `DomTransitionConfig`: `attribute: DomAttribute` →
  `property: PresentationProperty`; `from`/`to`/`presented` are `DeclValue`.
  `DomTransitionEvent.attribute` → `property`.
- Scheduling moved into the style pass: a transition starts at the rendering
  update that detects the value change (browser timing), not at the write.
- The four transition longhand lists are bounded at **4 items**;
  over-capacity lists are rejected at admission (dropped with count in
  stylesheets, typed error in `set`/`set_all`).
- Interpolated values now transmit through inheritance mid-transition
  (CSS Cascade 5 §6.1: the transition tier wins the cascade).
- `set_transition` validates the payload against the property's grammar
  (`DomError::InvalidAttributeValue` on a kind mismatch).

## The parse-tightening table (D5)

The closed property table (68 properties, `PresentationProperty`) parses
typed values at the declaration boundary. These grammars tighten what v0.5
tolerated as free strings. Anything outside the table is unsupported by
policy (the D6 admission policy: a named consumer need, a spec citation
plus the Blink/Gecko reference, and a proof — file it).

| Property | v0.5 tolerated | v0.6 grammar |
| --- | --- | --- |
| `isolate` | any bool/string | `auto` \| `isolate` |
| `overflow` | free string | `visible` \| `scroll` \| `auto` \| `hidden` \| `clip` |
| `flex-direction` | free string | `row` \| `row-reverse` \| `column` \| `column-reverse` |
| `flex-wrap` | free string | `nowrap` \| `wrap` \| `wrap-reverse` |
| `justify-content` | free string | `start` \| `end` \| `flex-start` \| `flex-end` \| `center` \| `space-between` \| `space-around` \| `space-evenly` |
| `align-items` / `align-self` | free string | `start` \| `end` \| `flex-start` \| `flex-end` \| `center` \| `stretch` (+ `auto` for `align-self`) |
| `align-content` | free string | align-items set + `space-between` \| `space-around` \| `space-evenly` |
| `transform` | free string | `none` \| `translateX(<px>)` \| `translateY(<px>)` \| `translate(<px>, <px>)` |
| `font-weight` | free string | `normal` \| `bold` \| `bolder` \| `lighter` \| `100`–`900` (steps of 100) |
| `font-style` | free string | `normal` \| `italic` |
| `outline-width` / `outline-offset` | px string | `unset` \| `0` \| `<px>` \| `<em>` |
| `border-radius` | free-form string (px-only consumption) | 1–4 lengths, CSS corner mapping, expands to four per-corner longhands |
| `border-color` | per-side fallback string | 1–4 colors, expands to four per-side longhands |
| `gap` | standalone property | shorthand of `row-gap` + `column-gap` (`normal` = `0px`; `unset` rejected) |
| `flex` | stored verbatim | expands to `flex-grow` + `flex-shrink` + `flex-basis` |
| `flex-flow` | stored verbatim | expands to `flex-direction` + `flex-wrap` (omitted = initial) |
| `place-*` | stored verbatim | expands to the align/justify pairs (`place-items`/`place-self` keep the align half; the justify half is validated and dropped — not a flex property) |
| `transition-*` lists | any length comma lists | ≤ 4 items per list |
| `border-<side>` | any string | one length (`border-top: 2px` → `border-top-width`) |

Other notes:

- Authored `0` lengths normalize to `0px`; numbers/lengths serialize in
  their canonical computed form.
- `em` resolves at style time: `font-size` em against the parent's computed
  font-size; every other property's em against the element's own (CSS
  Values §em). Em radii now resolve (v0.5 mapped them to `0.0` in paint).
- Text-valued properties (`font-family`, `transform`, transition lists)
  intern in a bounded arena; `TextKey`s are store-relative and never
  serialized.

## Behavior notes (functional, not parse)

- **`style_mut` targets elements only** (`InvalidAttributeTarget` for
  document/fragment/text nodes). Fragment (detached) trees cannot hold
  declarations.
- **Behavior sheets are typed** (`Declaration` lists in element
  descriptors); text-valued properties are unsupported in behavior sheets
  (ignored; admission via the policy).
- **Widget control defaults remain crate-owned.** One documented edge: an
  authored `border-radius: 0px` on a range control now reads as the
  absent-value pill default (the pill default belongs in the behavior-sheet
  tier; tracked as a follow-up).
- **`transform` transitions no longer schedule.** v0.5 presented a
  delayed-step; the typed translate group (`[f32; 2]`) makes real
  interpolation a small follow-up — admission via the policy if you need it.

## For the sister PR (stream-pro2 conversion)

- **337 `set_attr_str` call sites**: mechanical conversion to
  `style_mut().set(...)` for presentation names; content attributes
  (`class`, `id`, `type`, `value`, `dir`, widget state) stay on
  `set_attr*`.
- **13 `compute_style_map` call sites, all `#[cfg(test)]`** (audited for
  PLAN.md open question 9 — none are per-frame): drop the second argument;
  map keys become `PresentationProperty`.
- No `apply_styles`, no `StyleRule`, no presentation `DomAttribute`
  variants — those call sites simply stop compiling; convert them to the
  table above.
