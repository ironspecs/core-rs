# style

Retained stylesheet storage, selector parsing and matching, specificity, and
cascade ordering for `ui-dom`.

## Purpose and scope

This module turns authored stylesheet text into retained, indexed rules and
answers "which rules style this element right now" for the style-recompute
visitor's typed winners fold (`domains/visitor/winners_fold.rs`) and the
debug-priced one-off query API (`RootDom::compute_style_map`). It owns
everything up to and including cascade ordering. It does not own inheritance
(`domains/visitor`), resolution of declared values into the seven computed
groups (the shared resolver in `computed/project.rs`, driven by the winners
fold), or the computed read surface (`RootDom::computed_style`).

## Ownership boundaries

- `style_runtime.rs` owns `StyleRuntime`: the rule store, the `DeclStore`
  arena (all typed declaration blocks + the text interner), and every index.
  It never parses selector syntax itself; rule compilation delegates to the
  parent module's parsers.
- `mod.rs` owns the selector subset grammar, the stylesheet block parser,
  selector matching, specificity, precedence, the `ComputedStyleMap` type, and
  the rule identity hash over typed declarations. It never stores rules or
  blocks; the retained recompute visitor folds typed winners itself in
  `domains/visitor/winners_fold.rs` (spine 3.3g-2a).
- `decl_value.rs` owns `decl_value_to_dom_value` (the surviving value formatter
  the debug-priced `compute_style_map` uses to project winners to the public
  wire shape, moved here from the deleted `decl_bridge.rs` at spine 3.4, when
  the presentation variants left `DomAttribute`) and its CSS-string sibling
  `decl_value_to_css_string` (plus the `format_color_rgba` color serializer),
  which `canonical_style_snapshot` uses to keep important declarations
  replay-faithful for every value kind.
- `style_rule_origin.rs` owns `StyleRuleOrigin`, the crate's stylesheet-origin
  identity (`key` + `priority`).
- `style_mut.rs` owns the programmatic declaration write API (the two D5
  contracts) and `RootDom` owns the inline declaration arena (element-owned
  blocks keyed by `NodePresentation::decl`; freed with their owner). The
  winners fold reads the inline block as the top cascade tier.
- The typed declaration model owns itself, one type per file:
  `presentation_property.rs` (the closed 69-property vocabulary — 68 at v0.6.0,
  growing via admission per the D6 policy — with its
  per-property name, group, inherited flag, and grammar), `keyword.rs` (the
  closed keyword vocabulary), `cascade_tier.rs` (`CascadeTier`, the
  origin-and-importance ladder projected onto `Precedence`'s first field),
  `decl_value.rs` (`DeclValue`/`DeclLength`/`TextKey`/`Precedence`),
  `declaration.rs` (one typed declaration), `parsed_declaration.rs` (one typed
  declaration plus its `!important` flag, the strict-parse result),
  `property_set.rs` (the membership bitset), `text_interner.rs` (the text
  arena), `decl_block.rs` (the ordered, property-deduplicated block with its
  A10 minimal-edit upsert and a parallel importance bit vector),
  `decl_store.rs` (the block arena + generational
  keys),   `decl_drop_report.rs` (the parse report types), and `decl_parse.rs`
  (A1: the per-property closed grammars, drop-and-continue block parsing,
  the system's single shorthand-expansion point, the `!important` marker
  stripper, and the name-based D5 admission surface `name_is_presentation`).
- The computed-value store owns itself, one file per behavior-bearing type
  under `computed/`: seven `computed_*_group.rs` files (the S1.1
  inheritance-grouped `Copy` structs), `box_length.rs` (the `BoxLength`
  computed/used length split — `Auto | Px | Percent`, CSS Cascade §4.5),
  ten `*_kw.rs` files (the box/flex
  keyword vocabularies, one enum per file), `computed_groups.rs` (the seven
  held together), `node_presentation.rs` (inline block key + computed groups +
  last-pass `previous_computed` baseline + transition longhands),
  `resolved_transition_longhands.rs` (the scheduler slot),
  `group_dirty_classification.rs` (A7's static group-diff table), and
  `field_mapping.rs` (the field-level `for_each_moved_field` compare the
  transition scheduler keys on). These are **pure data** plus `initial()` and
  tiny field helpers; resolution logic lives in the step-3.3 resolver
  (`project.rs`), and the used-value percentage resolution lives in the
  layout-facing readers (`domains/layout/style.rs`), never here.

## Architectural approach

- **Retained rule store + declaration arena.** `SlotMap<StyleRuleKey,
  CompiledRule>` holds rules under stable generational keys; each rule holds a
  `DeclBlockKey` into the arena `DeclStore` (a `SlotMap` of `DeclBlock`s plus
  a text interner). Origin unload is a tombstone: the entry stays, `active`
  flips false, the indexes keep their keys, and the block stays in the arena
  (reclamation is open question 6).
- **Typed declarations, parsed drop-and-continue.** A1 (`parse_declaration_block`)
  parses each rule's declaration list into typed `Declaration`s (shorthands
  expand to longhands at parse), dropping malformed declarations with a
  counted report (CSS 2.1 §4.1.7) — never silently, and never a whole-block
  rejection.
- **Inverted indexes on the right-most compound.** `id_index`, `class_index`,
  `tag_index` (`HashMap<String, Vec<StyleRuleKey>>`) and `universal_index`
  give indexed candidate lookup — the same bucketing shape as Blink's
  `RuleSet`. `origin_index` makes unload proportional to the origin's rule
  count; `identity_index` gives hash-bucket duplicate-load dedup (the hash
  covers origin, specificity, selector parts, and the typed declarations).
- **Descendant-only selector AST.** `Vec<SimpleSelector>`, matched
  right-to-left with a parent-chain scan per preceding compound. The supported
  grammar is a closed subset: `#id`, `.class` (multi-class allowed), `tag`,
  `tag.class`, `*`, descendant combinators, and the four pseudo-classes
  `:hover`, `:active`, `:focus`, `:focus-visible`. Everything else — child and
  sibling combinators, selector lists, attribute selectors, pseudo-elements —
  is rejected at load with `DomError::InvalidSelector`.
- **Specificity as an ordered (a, b, c) triple.**
  `SelectorSpecificity { id, class, tag }` derives `Ord`; field declaration
  order yields the CSS lexicographic comparison. Pseudo-classes count into
  `class`.
- **Cascade = sort-then-fold.** Matched rules sort by
  `(origin.priority, specificity, order)`, where `order` is a monotonic
  per-runtime counter capturing source order, then the winners fold
  (`domains/visitor/winners_fold.rs`) folds their typed declaration blocks —
  later-or-higher precedence wins. Element pseudo-state declarations (from
  `DomStyleObject`) are typed `Declaration`s folded *before* matched rules, so
  author stylesheet rules override them: they act as a widget-behavior sheet,
  not as inline style. Text-valued properties (font-family, transform,
  transition longhands) are not supported in behavior sheets — their
  interner-relative `TextKey`s have no owning store, so the fold ignores them.
- **The typed winners fold is the only channel to computed groups (spine
  3.3g-2a).** The fold resolves the seven groups and the transition-longhand
  slot directly from the typed winners (seed from the parent's merged groups)
  and writes them onto the node record. The legacy attr-projection channel
  (`project_node_presentation` / `reproject_node_presentation` / `apply_styles`
  and the winners→legacy-map rebuild) is dead; presentation attributes error at
  the D5 enforcement gate and route through `style_mut`.
- **Group-copy inheritance seeding (spine 3.3d).** `computed/project.rs::
  parent_inherited_groups` seeds the nearest ancestor element's three inherited
  groups (font/text/color copied, the four non-inherited reset to initial).
  This closes the pre-redesign gap where line-height/white-space/overflow-wrap/
  word-break and the font longhands were context-carried or approximated
  instead of inherited through their groups.
- **Transition re-key (spine 3.3e).** The transition scheduler's retained
  objects (`DomTransition`/`DomTransitionConfig`) are keyed by
  `PresentationProperty` and interpolate typed `DeclValue`s (`Length(Px)`,
  color channels, `Number`, and the typed `Translate` pair lerp; every other
  kind — keywords, text, and the `none` ↔ translate crossing — steps). The
  overlay merge
  in `DomNode::computed_*` re-keys from the transition's property to its group
  field, and the overlay becomes the inheritance seed (CSS Cascade §6.1): the
  transition tier wins the cascade, so a parent's interpolated inherited value
  (font/text/color) is the cascaded value and inherits to descendants.
- **Pass-driven transition scheduling (spine 3.3g-1b, D2 one-timing).**
  Transition scheduling lives in the style pass: after the fold resolves the
  seven groups and the transition-longhand slot, the pass diffs them against
  the last completed pass's groups (`NodePresentation::previous_computed`) at
  field level (`computed/field_mapping.rs::for_each_moved_field`), maps each
  moved field to its `PresentationProperty`, resolves the slot's spec
  (`transition_spec_for_longhands` — `all`/`none`/name match, last-match wins,
  list-repeat), and installs a transition whose `from` is the last-pass
  presented value and `to` is the new field. The write path
  (`style_mut` / the `style`-attribute route) only upserts the inline block and
  marks the node style-dirty, so a direct presentation write reaches the pass
  instead of scheduling synchronously; the same pass applies A7's group-diff
  classification (`GroupDirtyClassification::diff`).

## Invariants, rules, and decisions

- The presentation vocabulary is closed at 69 properties (the value table in
  `PLAN.md`; 68 at v0.6.0, growing via admission per the D6 policy), each with
  a closed grammar. A property or grammar form enters
  only through the admission policy: a named consumer need, a spec citation
  plus the Blink/Gecko algorithm reference, and a proof in the same change.
  Unadmitted declarations are dropped at parse (counted), never silently
  accepted and never fatal.
- Shorthand expansion happens exactly once in the system, at declaration-parse
  time in `decl_parse.rs` (CSS Cascade 5 §3: a shorthand sets all of its
  longhands exactly as if expanded in place). The cascade and every consumer
  only ever see longhands. The admitted shorthands are `border-radius`,
  `border-color`, `gap`, `flex`, `flex-flow`, `place-content`, `place-items`,
  `place-self`, `transition`, `margin`, `padding`, and the four
  `border-<side>` width aliases (width-only in this vocabulary; style/color
  components are not admitted). The `margin` shorthand also admits `auto` per
  token (`margin: 0 auto`); `padding` does not (auto is invalid there).
- Declaration parsing is drop-and-continue (CSS 2.1 §4.1.7): a malformed
  declaration is dropped and parsing continues. Drops are always observable —
  counted in the `StylesheetLoadReport` and in the runtime's monotonic
  `declarations_dropped` counter (read via
  `RootDomContext::declaration_drop_count`); `RootDomContext::
  declaration_block_count` exposes live arena blocks as a level accessor over
  retained state. This is a scoped, adjudicated exception to the repo's
  fail-fast rule for boundary data (see AGENTS.md); single-declaration APIs
  stay strict.
- The precedence model is `{ tier, origin_priority, inline, specificity, order }`
  — the `Precedence` record in `decl_value.rs`, whose `tier` field (`CascadeTier`
  in `cascade_tier.rs`) projects CSS Cascade 5 §6.1's origin-and-importance
  ladder: `ImportantAuthor > NormalAuthor > NormalUa` (the behavior sheet's
  `NormalUa` rung), so important author declarations beat all normal
  declarations including normal inline style. The `inline` flag sorts above any
  selector per css-style-attr §4. (The matched-rule pre-sort uses a narrower
  `(priority, specificity, order)` tuple, which omits the tier because rules
  are never inline and the tier is a per-DECLARATION importance flag; inline
  declarations enter the fold through the inline block at the top precedence.)
  `!important` is admitted (F stage 1); there is no built-in UA stylesheet
  (consumers may model one as a low-priority origin), and no cascade layers.
  These are known divergences from CSS Cascading and are pinned as labeled
  off-spec proofs in `tests/layout_proofs/`.
- `StyleRuntime::revision` is boundary change detection only: a `RootDom`
  snapshot-compares it once per update to dirty the tree when the stylesheet
  *set* changes across the shared-context seam. It is not a per-node or
  per-rule version and must not grow traversal consumers — see
  `domains/visitor/README.md` for the dirty-bit doctrine.
- Stylesheet text is boundary data: parse and match failures are typed errors
  (`DomError`), never panics.
- `StyleRuntime::load_stylesheet` is atomic (buffer-then-commit): selector
  parsing, right-most extraction, specificity, declaration parsing, the
  identity hash, and the dedup check happen in a staging phase that mutates
  nothing retained; the rule store, the indexes, `order_counter`, and
  `revision` commit only after every block has parsed. A typed `Err` therefore
  means "nothing happened" to the rule store. Declaration drops still
  accumulate on a failing load (the G drop-report contract — drops are
  observations of malformed input, not rule-store mutations).
- Pseudo-state rule counts (`hover_rule_count` and friends) are kept exact so
  `uses_pseudo_state` answers without scanning rules: incremented at index
  time, decremented on tombstone, asserted against underflow.
- Selector parsing, specificity, and precedence live here and nowhere else;
  the visitor and layout must not re-implement them.
