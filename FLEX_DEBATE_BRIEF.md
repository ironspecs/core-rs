# FLEX_DEBATE_BRIEF.md — the PR 3 third-party work order

Status: `CONVENED 2026-10-19; amended the same day under the round-2
and round-3 reviews (PLAN.md decision log) — implementation unlocks
when every obligation below returns a named verdict`. Subject: `FLEX_MAPPING.md`
(amended), `PLAN.md` PR 3 (rewritten). This brief is the work order;
those documents carry the substance. Rule: agreement is per obligation,
BY NAME — a general assent is not agreement.

**Assignee and independence (added per round 2, finding 4).** The
third-party reviewer: the author of the 2026-10-19 review chain (the
graceful-gate review, the mapping amendments, the round-2 review of
this brief). Independence criteria, required of any substitute: NOT the
architect, NOT the implementation agents; demonstrably able to read
Chromium/Gecko sources against our tree; holds kill-criterion
authority. Author and reviewer seats are distinguished in PLAN.md's
decision log by title.

**Residual conflict and its rule (added per round 3).** The named
reviewer authored the round-1 amendments that three obligations audit:
(c) audits the §1 classification the reviewer demanded, (e) adjudicates
the kill criterion the reviewer restated, (f) covers the §6
verification-burden plan the reviewer substituted. Independence of the
AUTHOR is structural; independence of the AMENDMENTS is not. Rule:
obligations (c), (e), and (f) require EITHER co-signature by the
product owner on each verdict OR execution by a fresh seat with no
review-chain context; the owner names which before those three
obligations execute, and the choice is recorded in PLAN.md's decision
log. Obligations (a), (b), and (d) are conflict-free — they verify
engine sources, enumerated scenarios, and budget mechanics. Note on
(b): its targets (the run-aggregation cursors, the refold bound) are
also reviewer-authored, but its verdict form is adversarial by
construction — a failing scenario or a scoped none-found — and a false
none-found is caught at step 1's gate when the pins materialize, so it
carries no ratchet direction; if the owner disagrees, the same rule
extends to it.

**Evidence corpus (added per round 2, finding 1; rationale corrected
per round 3).** The fetched Blink/Gecko sources live at
`.tmp/engine-corpus/` (GITIGNORED — the product owner's policy:
licensed third-party source stays out of this repo on hygiene, scope,
and reproducibility grounds; both upstream licenses permit vendoring
under conditions this repo declines to take on) with `MANIFEST.md`
carrying per-file upstream URLs,
resolved revision hashes, fetch date, and revision-pinned re-fetch
instructions. Every file:line verdict below cites against the
manifest's revisions; a cold reviewer re-fetches per the manifest if
the corpus is absent.

**Sequencing (round 2, finding 8; updated per round 3).** Obligations
(a), (b), and (d1) are executable today — (a) needs the engine corpus,
the rest need only our tree. Obligations (c), (e), and (f) execute once
the product owner names the independence rule above. Nothing else
blocks local work.

## Obligation (a) — source spot-check

Verify the five skeleton claims (FLEX_MAPPING §2.1) FROM the corpus
sources, not from the mapping document's paraphrase. Verdict per claim:
CONFIRMED, CONFIRMED-WITH-DELTA (semantically equivalent, structurally
different — with the delta named; added per round 2: a binary verdict
invites overfitting claims to anchors), or REFUTED — each with
file:line from the source itself.

| # | Claim | Blink anchor | Gecko anchor |
|---|---|---|---|
| 1 | Item formation happens BELOW the algorithm (anonymous runs pre-wrapped; abs-pos/none excluded from item slots) | `layout_block.cc:147-171`; `flex_child_iterator.cc:18-24` | `nsCSSFrameConstructor.cpp:8134` (+ ~8180 whitespace drop) |
| 2 | Content-box base; automatic minimum FOLDED INTO the hypothetical clamp | `flex_item.h:47-48,104-112`; `flex_layout_algorithm.cc:1060-1178` | `nsFlexContainerFrame.cpp:660-698` |
| 3 | §9.7 seed at hypothetical; freeze ONE violation class per iteration by signed total | `line_flexer.cc:26-43,90-181` | `nsFlexContainerFrame.cpp:3050-3176` |
| 4 | Cross sizes determined AFTER main; `CanMainSizeInfluenceCrossSize` flag semantics | `flex_layout_algorithm.cc:1524-1612` | `nsFlexContainerFrame.cpp:2546-2600,4611-4645` |
| 5 | Negative free-space alignment: 0-clamp / safe collapse, never unsafe center | `flex_layout_algorithm.cc:1660-1715` | `nsFlexContainerFrame.cpp:3553-3581,3736-3762` |

## Obligation (b) — counterexample hunt

Attack, from the enumerated corpus (the 424-proof suite, the two-axis
enumeration, the composition families), for a FAILING scenario against:

- the §3.2 run-aggregation cursors: heterogeneous fonts inside one run;
  the widest unbreakable segment spanning a node boundary (adjacent
  spans, no intervening space); a whitespace-only run under `pre-wrap`
  (must form an item) vs under `normal`/`pre-line` (must drop);
- the refold bound: nested `main_influences_cross` flags (text inside a
  nested flex item inside a shrink context) — is once-per-level
  sufficient, and is the level count what the metric would observe?

Verdict per attack: the scenario, expected, actual — or "none found"
with the search scope stated.

## Obligation (c) — classification audit

Attack every §1 NO/PARTIAL row for a hidden structural dependency that
would make an in-place pin-flip landing dishonest. REQUIRED specific
probe (the architect's countersign, PLAN.md decision log 2026-10-19):
the paint-order NO classification assumes the order-modified sequence
is consumable by paint TODAY — `line_collection_items` exists
(`flex.rs:177/388/1639`), but whether the solve's sequence survives
past relayout to paint time is unverified. Verdict per row: holds, or
reclassified with the dependency named.

## Obligation (d1) — budget POLICY, ratified now (split per round 2)

Ratify, against FLEX_MAPPING §3.4: (i) the metric DEFINITIONS (flex
nodes measured, flex nodes placed, flagged refolds per level — landing
with the structural step); (ii) the construction bound (total intrinsic
measurements per dirty update ≤ 2× the up-fold count — each subtree
refolds at most once per nesting level); (iii) the within-update memo
fallback (context-owned, keyed by `(node, resolved width)`, reset per
update) is designed rather than improvised and does not violate the
no-speculative-caches doctrine; (iv) the ceiling AS POLICY: ≤ 50% of
measured flex nodes flagged-refolding in the pinned scenario, breach
landing the memo; (v) the scenario DEFINITION (the ceiling's subject —
authored-input shape, pinned at step 1): a wrapping row container at
240px content width holding 12 short text-bearing items plus 4 nested
flex items each with a text child, with one text mutation per update —
the text-heavy-in-shrink-context stress the refold flag exists for.

## Obligation (d2) — budget BASELINE, at step 1's gate (split per round 2)

The scenario from (d1)(v) lands with the structural step's perf pins;
the observed refold fraction is recorded in PLAN.md's decision log at
that gate; breaching the (d1)(iv) ceiling lands the memo with its own
perf pins. (Round-2 finding 2: the number cannot be observed before
step 1, so no ratification of an observed value is possible or required
now — ratifying one would be theater.)

## Obligation (e) — kill-criterion adjudication, with the fire path (round 2, finding 3)

Confirm the restated criterion (PLAN.md PR 3, question 5) is
falsifiable in both directions, then adjudicate it against the doctrine
evidence — the recursion and allocation inventory in the hot path:
per-container `Vec<MeasuredFlexItem>` (`flex.rs:406-434`), the
`HashMap` re-join (`flex.rs:1894-1923`), the recursive auto-min walk
(`flex.rs:1252-1367`), the staged double-layout of every child
(`flex.rs:2365-2480`). Verdict: FIRES (name the branch and the
evidence) or does not fire.

**Fire-path rules (binding):**
- **Per-claim consequence:** a REFUTED claim N in obligation (a)
  re-maps the phases resting on N (the mapping document is corrected in
  the same decision). The criterion fires only when the refuted set
  defeats the doctrine's OWN evidence — and that evidence (the
  recursion/allocation inventory above) is LOCAL and engine-independent,
  so no skeleton-claim refutation alone can fire branch (a). This rule
  exists so neither ratchet direction is spinnable.
- **Branch (a) firing** amends the layout-pass doctrine entry itself —
  which binds PR 5 as well as PR 3 — so the same decision must
  re-scope PR 5 explicitly.
- Verdict values per claim per obligation (a): CONFIRMED /
  CONFIRMED-WITH-DELTA / REFUTED.

## Obligation (f) — substitute-surface coverage (added per round 2, finding 5)

Against the CONFIRMED §2.1 claims (after (a)): confirm FLEX_MAPPING §6's
phase-family plan maps onto them with NO phase unpinned — each
confirmed skeleton phase has a named proof family with spec section and
WPT provenance, including phases that move no registered row
(line-formation invariants, greedy break-never-before-first-item,
gap-once-per-adjacency, the single-line cross short-circuit). Any
unpinned phase is a finding with the family owed.

## The debate's required outputs

1. The per-obligation verdicts (a)–(f), each named.
2. **Sequencing pulls:** which §1 NO/PARTIAL rows, if any, pull forward
   as in-place pin-flip landings on the current solver (accepting
   double work), each recorded in FLEX_MAPPING §4. None by default.
3. **The refold budget policy ratification** (obligation d1) — and at
   step 1's gate, the observed baseline (obligation d2).
4. **Grid's dated arc decision** (PLAN.md PR 3, question 6 — REQUIRED;
   the recorded recommendation: enter immediately after the fix
   sequence, before PR 4).
5. **The step-10 admission scope** (added per round 2, finding 6;
   deferral closed per round 3): which admission candidates land
   in-arc. Default recorded: `margin: auto` §8.1 lands in-arc first;
   `flex-basis` percentages, intrinsic size keywords, and `aspect-ratio`
   are deferred to a post-sequence consumer re-check; all remaining
   candidates (baseline alignment, `visibility: collapse`) are deferred
   to the same re-check. Overrule with rationale; silence adopts the
   default.

Implementation unlocks at FLEX_MAPPING §4 step 1 when all five outputs
are recorded in PLAN.md's decision log.
