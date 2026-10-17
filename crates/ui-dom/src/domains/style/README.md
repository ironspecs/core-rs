# style

Retained stylesheet storage, selector parsing and matching, specificity, and
cascade ordering for `ui-dom`.

## Purpose and scope

This module turns authored stylesheet text into retained, indexed rules and
answers "which rules style this element right now" for the style-recompute
visitor (`domains/visitor/style_recompute.rs`) and the one-off query API
(`RootDom::compute_style_map`). It owns everything up to and including cascade
ordering. It does not own inheritance (`domains/visitor`), resolution of
declared values to typed units (consumers such as `domains/layout/style.rs`
and the inherited-style context), or application of style onto retained nodes
(`RootDom::apply_styles`).

## Ownership boundaries

- `style_runtime.rs` owns `StyleRuntime`: the rule store and every index. It
  never parses selector syntax itself; rule compilation delegates to the
  parent module's parsers.
- `mod.rs` owns the selector subset grammar, the stylesheet block parser,
  selector matching, specificity, precedence, the `ComputedStyleMap` type,
  and the public query functions (`matched_style_rules_for_element`,
  `apply_pseudo_state_style_declarations`). It never stores rules.
- `style_rule_origin.rs` owns `StyleRuleOrigin`, the crate's stylesheet-origin
  identity (`key` + `priority`).

## Architectural approach

- **Retained rule store.** `SlotMap<StyleRuleKey, CompiledRule>` holds rules
  under stable generational keys. Origin unload is a tombstone: the entry
  stays, `active` flips false, and the indexes keep their keys.
- **Inverted indexes on the right-most compound.** `id_index`, `class_index`,
  `tag_index` (`HashMap<String, Vec<StyleRuleKey>>`) and `universal_index`
  give indexed candidate lookup — the same bucketing shape as Blink's
  `RuleSet`. `origin_index` makes unload proportional to the origin's rule
  count; `identity_index` gives hash-bucket duplicate-load dedup.
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
  per-runtime counter capturing source order, then fold into
  `ComputedStyleMap = BTreeMap<DomAttribute, DomAttributeValue>`; later insert
  wins. Element pseudo-state declarations (from `DomStyleObject`) are applied
  *before* matched rules fold in, so author stylesheet rules override them:
  they act as a widget-behavior sheet, not as inline style.
- **Stringly-typed declared values.** Declarations keep `DomAttributeValue`
  (mostly `String`) through matching and storage. Unit and keyword resolution
  is deferred to consumers (`parse_authored_length`, layout's `NodeStyle`,
  the inherited-style context). This module never resolves `em`, `px`, or
  property keywords.

## Invariants, rules, and decisions

- The precedence model is exactly `(origin.priority, specificity, order)`.
  There is no `!important`, no built-in UA stylesheet (consumers may model one
  as a low-priority origin), and no cascade layers. These are known
  divergences from CSS Cascading and are pinned as labeled off-spec proofs in
  `tests/layout_proofs/`.
- `StyleRuntime::revision` is boundary change detection only: a `RootDom`
  snapshot-compares it once per update to dirty the tree when the stylesheet
  *set* changes across the shared-context seam. It is not a per-node or
  per-rule version and must not grow traversal consumers — see
  `domains/visitor/README.md` for the dirty-bit doctrine.
- Stylesheet text is boundary data: parse and match failures are typed errors
  (`DomError`), never panics.
- Pseudo-state rule counts (`hover_rule_count` and friends) are kept exact so
  `uses_pseudo_state` answers without scanning rules: incremented at index
  time, decremented on tombstone, asserted against underflow.
- Selector parsing, specificity, and precedence live here and nowhere else;
  the visitor and layout must not re-implement them.
